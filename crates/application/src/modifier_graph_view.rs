use std::collections::{HashMap, HashSet};

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex, IndexPath, StyledExt, TestSupportExt};
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
use gpui_kit::*;
use project::modifier::{
    ConstantMode, GraphBlendMode, MathOp, ModifierGraph, ModifierNode, NodeKind, WaveType,
};
use project::StockPlugin;

use crate::state::EditorState;

/// Default pixel height of the top header area (Row 1: 42px + Row 2: 34px + border: 1px = 77px).
/// Only the boot default — the real canvas origin is measured at paint time
/// (the toolbar row can wrap, growing the header).
pub const HEADER_HEIGHT: f32 = 77.0;

/// Display label for a socket: Blend's `a`/`b` read as background/foreground
/// (wire names stay a/b/weight so old saved graphs load untouched).
fn socket_display_label(kind: Option<&NodeKind>, socket: &'static str) -> &'static str {
    if matches!(kind, Some(NodeKind::Blend { .. })) {
        match socket {
            "a" => "background",
            "b" => "foreground",
            _ => socket,
        }
    } else {
        socket
    }
}

/// Map window-space cursor coordinates to header-relative coordinates
/// (no pan, no measured header growth). Kept for delta-only reads.
#[inline]
pub fn event_to_canvas_pos(pos: Point<Pixels>) -> (f32, f32) {
    (pos.x / px(1.0), (pos.y / px(1.0)) - HEADER_HEIGHT)
}

/// Open a dedicated modifier graph editor window for a property on a layer.
pub fn open_modifier_graph_window(
    editor_state: Entity<EditorState>,
    layer_id: String,
    prop_path: String,
    cx: &mut App,
) {
    let bounds = Bounds::centered(None, size(px(1080.), px(720.)), cx);
    let title = if prop_path.starts_with("graph:") {
        format!(
            "Effect Node Graph — Effect {} (Layer {})",
            prop_path.trim_start_matches("graph:"),
            layer_id
        )
    } else {
        format!("Modifier Graph — {} (Layer {})", prop_path, layer_id)
    };
    let es = editor_state.clone();
    let lid = layer_id.clone();
    let ppath = prop_path.clone();

    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |window, cx| {
            window.activate_window();
            window.set_window_title(&title);
            Theme::change(ThemeMode::Dark, Some(window), cx);
            let view = cx.new(|cx| ModifierGraphView::new(es, lid, ppath, window, cx));
            cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
        },
    );
}

/// Active wire connection drag state.
#[derive(Clone, Debug)]
pub struct WireDragState {
    /// True if drag started from an Input socket seeking an Output socket;
    /// false if started from an Output socket seeking an Input socket.
    pub is_from_input: bool,
    pub node_id: String,
    pub socket_name: String,
    /// Current canvas-space cursor position
    pub cur_x: f32,
    pub cur_y: f32,
    /// Initial canvas-space click position to distinguish stationary click vs drag
    pub start_x: f32,
    pub start_y: f32,
}

/// Standalone interactive visual node graph editor for modifier graphs.
pub struct ModifierGraphView {
    focus_handle: FocusHandle,
    editor_state: Entity<EditorState>,
    layer_id: String,
    prop_path: String,
    pub graph: ModifierGraph,
    /// True when editing an inline effect graph (effect:"graph:" paths):
    /// renames the Content node and applies effect-specific chrome.
    is_effect_graph: bool,
    _subscription: Subscription,
    /// Dragging a node: `(node_id, initial_canvas_x, initial_canvas_y, initial_node_x, initial_node_y)`
    dragging_node: Option<(String, f32, f32, f32, f32)>,
    /// Connecting a wire state
    pub connecting_wire: Option<WireDragState>,
    /// Selected node ID
    selected_node: Option<String>,
    /// Canvas pan offset (X, Y)
    pub(crate) pan_offset: (f32, f32),
    /// Window-space origin of the canvas, measured at paint time. Cursors
    /// map through this instead of a hardcoded header height.
    canvas_origin: std::sync::Arc<std::sync::Mutex<(f32, f32)>>,
    /// Canvas panning: `(initial_canvas_x, initial_canvas_y, initial_pan_x, initial_pan_y)`
    pub(crate) panning_canvas: Option<(f32, f32, f32, f32)>,
    /// Scrubbing param: `(node_id, param_name, last_mouse_x, original_val)`
    scrubbing_param: Option<(String, String, f32, f32)>,
    /// True once the active param scrub moved (a press without movement
    /// opens keyboard entry instead, like timeline value pills).
    scrub_moved: bool,
    /// Param currently under keyboard entry: `(node_id, param_name)`.
    pub(crate) editing_param: Option<(String, String)>,
    /// Retained single-line editor for `editing_param`.
    pub(crate) param_editor: Option<Entity<InputState>>,
    /// Subscription for the retained editor.
    param_editor_sub: Option<Subscription>,
    /// Retained Blend-mode Combobox states, keyed by blend node id.
    blend_combos: HashMap<String, Entity<ComboboxState<SearchableVec<String>>>>,
    /// Confirm subscriptions for the retained blend Comboboxes.
    blend_combo_subs: HashMap<String, Subscription>,
    /// Node palette sidebar section open/closed (Value/Color/Space/Bus/Driver).
    /// Sections start collapsed: the sidebar is a clean list of categories.
    palette_open: [bool; 5],
}

impl ModifierGraphView {
    pub fn new(
        editor_state: Entity<EditorState>,
        layer_id: String,
        prop_path: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let is_effect_graph = prop_path.starts_with("graph:");
        let graph = editor_state.update(cx, |s, _cx| {
            s.ensure_layer_modifier_graph(&layer_id, &prop_path)
        });

        let sub = cx.observe(&editor_state, |_this, _state, cx| {
            cx.notify();
        });

        Self {
            focus_handle,
            editor_state,
            layer_id,
            prop_path,
            graph,
            is_effect_graph,
            _subscription: sub,
            dragging_node: None,
            connecting_wire: None,
            selected_node: None,
            pan_offset: (0.0, 0.0),
            canvas_origin: std::sync::Arc::new(std::sync::Mutex::new((0.0, HEADER_HEIGHT))),
            panning_canvas: None,
            scrubbing_param: None,
            scrub_moved: false,
            editing_param: None,
            param_editor: None,
            param_editor_sub: None,
            blend_combos: HashMap::new(),
            blend_combo_subs: HashMap::new(),
            palette_open: [false; 5],
        }
    }

    /// Window-space cursor → canvas space (pre-pan). Use for drag deltas only.
    fn cursor_canvas(&self, pos: Point<Pixels>) -> (f32, f32) {
        let (ox, oy) = *self.canvas_origin.lock().unwrap();
        (pos.x / px(1.0) - ox, pos.y / px(1.0) - oy)
    }

    /// Window-space cursor → panned canvas space (the space cards, wires and
    /// sockets are drawn and snap-tested in). Use for all absolute positions.
    fn cursor_canvas_abs(&self, pos: Point<Pixels>) -> (f32, f32) {
        let (x, y) = self.cursor_canvas(pos);
        (x + self.pan_offset.0, y + self.pan_offset.1)
    }

    /// Sync the current graph back into EditorState so changes immediately evaluate in the compositor.
    fn sync_to_state(&self, cx: &mut Context<Self>) {
        let lid = self.layer_id.clone();
        let path = self.prop_path.clone();
        let g = self.graph.clone();
        self.editor_state.update(cx, |s, cx| {
            s.set_layer_modifier_graph(&lid, &path, g);
            cx.notify();
        });
        cx.notify();
    }

    /// Ensure a retained Blend-mode Combobox exists per live blend node
    /// (created on first render; dropped when the node is deleted).
    fn ensure_blend_combos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let live: HashSet<String> = self
            .graph
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Blend { .. }))
            .map(|n| n.id.clone())
            .collect();
        self.blend_combos.retain(|k, _| live.contains(k));
        self.blend_combo_subs.retain(|k, _| live.contains(k));
        for id in &live {
            if self.blend_combos.contains_key(id) {
                continue;
            }
            let labels: Vec<String> = GraphBlendMode::ALL
                .iter()
                .map(|m| m.label().to_string())
                .collect();
            let idx = self
                .graph
                .get_node(id)
                .and_then(|n| match n.kind {
                    NodeKind::Blend { mode, .. } => Some(mode.index()),
                    _ => None,
                })
                .unwrap_or(0);
            let st = cx.new(|cx| {
                ComboboxState::new(SearchableVec::new(labels), vec![IndexPath::new(idx)], window, cx)
            });
            let view = cx.entity().clone();
            let nid = id.clone();
            let sub = cx.subscribe(
                &st,
                move |_, _, event: &ComboboxEvent<SearchableVec<String>>, cx| {
                    let vals = match event {
                        ComboboxEvent::Confirm(v) => v,
                        ComboboxEvent::Change(v) => v,
                    };
                    let Some(v) = vals.first() else {
                        return;
                    };
                    let Some(mode) = GraphBlendMode::from_label(v) else {
                        return;
                    };
                    // Deferred: the menu click/Enter fires while this view
                    // is mid-update (event dispatch runs inside it).
                    let view = view.clone();
                    let nid = nid.clone();
                    cx.defer(move |cx| {
                        view.update(cx, |this, cx| {
                            if let Some(n) = this.graph.get_node_mut(&nid) {
                                if let NodeKind::Blend { mode: m, .. } = &mut n.kind {
                                    *m = mode;
                                    this.sync_to_state(cx);
                                }
                            }
                        });
                    });
                },
            );
            self.blend_combos.insert(id.clone(), st);
            self.blend_combo_subs.insert(id.clone(), sub);
        }
    }

    /// Add a new node of the given kind at a convenient position.
    pub fn add_node(&mut self, kind: NodeKind, cx: &mut Context<Self>) {        let count = self.graph.nodes.len() + 1;
        let id = format!("node_{}_{}", kind.title().to_lowercase().replace(' ', "_"), count);
        // Position staggered in canvas view
        let offset_x = 220.0 + ((self.graph.nodes.len() as f32 % 5.0) * 40.0) - self.pan_offset.0;
        let offset_y = 120.0 + ((self.graph.nodes.len() as f32 % 5.0) * 35.0) - self.pan_offset.1;

        let node = ModifierNode::new(id, offset_x, offset_y, kind);
        self.graph.add_node(node);
        self.sync_to_state(cx);
    }

    /// Paste copied property link as a new DriverLink node into the graph.
    /// Can be called multiple times, creating multiple driver link nodes!
    pub fn paste_copied_property_link(&mut self, cx: &mut Context<Self>) {
        let copied = self.editor_state.read(cx).copied_property_link.clone();
        if let Some((driver_layer_id, driver_prop_path)) = copied {
            self.add_node(
                NodeKind::DriverLink {
                    driver_layer_id,
                    driver_prop_path,
                },
                cx,
            );
        }
    }

    /// Reset graph to default passthrough.
    pub fn reset_graph(&mut self, cx: &mut Context<Self>) {
        self.graph = ModifierGraph::default_passthrough();
        self.pan_offset = (0.0, 0.0);
        self.selected_node = None;
        self.connecting_wire = None;
        self.sync_to_state(cx);
    }

    /// Connect sockets.
    pub fn connect_sockets(
        &mut self,
        from_node: &str,
        from_socket: &str,
        to_node: &str,
        to_socket: &str,
        cx: &mut Context<Self>,
    ) {
        self.graph.connect(from_node, from_socket, to_node, to_socket);
        self.connecting_wire = None;
        self.sync_to_state(cx);
    }

    /// Disconnect an input socket.
    pub fn disconnect_socket(&mut self, to_node: &str, to_socket: &str, cx: &mut Context<Self>) {
        self.graph.disconnect_input(to_node, to_socket);
        self.sync_to_state(cx);
    }

    /// Remove a node by ID.
    pub fn remove_node(&mut self, node_id: &str, cx: &mut Context<Self>) {
        // Output node cannot be deleted
        if let Some(n) = self.graph.get_node(node_id) {
            if matches!(n.kind, NodeKind::Output) {
                return;
            }
        }
        self.graph.remove_node(node_id);
        if self.selected_node.as_deref() == Some(node_id) {
            self.selected_node = None;
        }
        self.sync_to_state(cx);
    }

    /// Attempt to complete a wire connection to a target position.
    /// Returns true if a connection was made.
    pub fn try_finish_wire_connection(
        &mut self,
        target_canvas_x: f32,
        target_canvas_y: f32,
        pan_x: f32,
        pan_y: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let wire = match &self.connecting_wire {
            Some(w) => w.clone(),
            None => return false,
        };

        let snap_radius = 48.0;
        let mut best_candidate = None;
        let mut best_score = f32::MAX;

        for node in &self.graph.nodes {
            if node.id == wire.node_id {
                continue; // Cannot connect a node to itself
            }
            let (nx, ny) = (node.pos_x + pan_x, node.pos_y + pan_y);
            let inside_node = target_canvas_x >= (nx - 10.0)
                && target_canvas_x <= (nx + 210.0)
                && target_canvas_y >= (ny - 5.0)
                && target_canvas_y <= (ny + 180.0);

            if !wire.is_from_input {
                // Dragging from OUTPUT socket -> seeking an INPUT socket
                for (idx, in_name) in node.kind.input_sockets().iter().enumerate() {
                    let sx = nx + 14.0;
                    let sy = ny + 41.0 + (idx as f32 * 26.0);
                    let dx = target_canvas_x - sx;
                    let dy = target_canvas_y - sy;
                    let dist = (dx * dx + dy * dy).sqrt();

                    let score = if inside_node {
                        dist
                    } else if dist <= snap_radius {
                        dist + 200.0
                    } else {
                        f32::MAX
                    };

                    if score < best_score {
                        best_score = score;
                        best_candidate = Some((node.id.clone(), (*in_name).to_string(), false));
                    }
                }
            } else {
                // Dragging from INPUT socket -> seeking an OUTPUT socket
                for (idx, out_name) in node.kind.output_sockets().iter().enumerate() {
                    let sx = nx + 186.0;
                    let sy = ny + 41.0 + (idx as f32 * 26.0);
                    let dx = target_canvas_x - sx;
                    let dy = target_canvas_y - sy;
                    let dist = (dx * dx + dy * dy).sqrt();

                    let score = if inside_node {
                        dist
                    } else if dist <= snap_radius {
                        dist + 200.0
                    } else {
                        f32::MAX
                    };

                    if score < best_score {
                        best_score = score;
                        best_candidate = Some((node.id.clone(), (*out_name).to_string(), true));
                    }
                }
            }
        }

        if let Some((target_node, target_socket, is_target_output)) = best_candidate {
            if is_target_output {
                // Wire was from input socket; target is output socket
                self.connect_sockets(&target_node, &target_socket, &wire.node_id, &wire.socket_name, cx);
            } else {
                // Wire was from output socket; target is input socket
                self.connect_sockets(&wire.node_id, &wire.socket_name, &target_node, &target_socket, cx);
            }
            self.connecting_wire = None;
            return true;
        }

        false
    }
}

impl Focusable for ModifierGraphView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ModifierGraphView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_blend_combos(window, cx);
        let (factor, base_val, evaluated_out, is_driven) = {
            let s = self.editor_state.read(cx);
            let factor = s.get_layer_progression_factor(&self.layer_id);
            let is_driven = s.is_layer_property_linked(&self.layer_id, &self.prop_path);
            let base = s.get_layer_property_live_value(&self.layer_id, &self.prop_path);
            let out = self.graph.evaluate_with_resolver(base, factor, &|lid, prop| {
                s.get_layer_property_live_value(lid, prop)
            });
            (factor, base, out, is_driven)
        };

        let entity = cx.entity().clone();

        // 1. Top Header & Toolbar
        let header = self.render_header(factor, base_val, evaluated_out, is_driven, &entity, cx);

        // 2. Node palette sidebar + canvas
        let canvas = self.render_canvas(&entity, cx);
        let palette = self.render_node_palette(&entity, cx);

        let ent_key = entity.clone();
        v_flex()
            .id("modifier_graph_root")
            .test_support()
            .size_full()
            .bg(cx.theme().background)
            .track_focus(&self.focus_handle)
            .on_key_down(move |event, _window, cx| {
                let mods = event.keystroke.modifiers;
                let ctrl = mods.control || mods.platform;
                let key = event.keystroke.key.to_lowercase();
                if ctrl && key == "v" {
                    ent_key.update(cx, |this, cx| {
                        this.paste_copied_property_link(cx);
                    });
                }
            })
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_stretch()
                    .overflow_hidden()
                    .child(palette)
                    .child(div().flex_1().child(canvas)),
            )
    }
}

impl ModifierGraphView {
    fn render_header(
        &self,
        factor: f32,
        base_val: f32,
        evaluated_out: f32,
        is_driven: bool,
        entity: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Div {
        let ent_paste_link = entity.clone();
        let ent_reset = entity.clone();
        let this_copied_link = self.editor_state.read(cx).copied_property_link.clone();

        v_flex()
            .w_full()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                // Row 1: Title and Live Readouts
                h_flex()
                    .w_full()
                    .h(px(42.))
                    .px_4()
                    .justify_between()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .w(px(18.))
                                    .h(px(18.))
                                    .items_center()
                                    .justify_center()
                                    .text_color(cx.theme().accent)
                                    .child(IconName::GitFork),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(cx.theme().foreground)
                                    .child(if self.is_effect_graph {
                                        "Effect Node Graph".to_string()
                                    } else {
                                        format!("Modifier Graph: {}", self.prop_path)
                                    }),
                            ),
                    )
                    .child(
                        // Live Readouts (Progression Factor, Base Value, Evaluated Result)
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(0x1e2230))
                                    .border_1()
                                    .border_color(rgb(0x3b82f6))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(rgb(0x93c5fd))
                                            .child("Layer Factor:"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_bold()
                                            .text_color(rgb(0x60a5fa))
                                            .child(format!("{factor:.3}")),
                                    ),
                            )
                            .child({
                                let mut pill = h_flex()
                                    .gap_1p5()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(if is_driven { rgb(0x2d1515) } else { rgb(0x28231a) })
                                    .border_1()
                                    .border_color(if is_driven { rgb(0xef4444) } else { rgb(0xd97706) });
                                if is_driven {
                                    pill = pill.child(
                                        div()
                                            .w(px(14.))
                                            .h(px(14.))
                                            .items_center()
                                            .justify_center()
                                            .text_color(rgb(0xef4444))
                                            .child(IconName::Link),
                                    );
                                }
                                pill.child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(if is_driven { rgb(0xfca5a5) } else { rgb(0xfcd34d) })
                                        .child(if is_driven { "Driver Value:" } else { "Base Value:" }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_bold()
                                        .text_color(if is_driven { rgb(0xef4444) } else { rgb(0xfbbf24) })
                                        .child(format!("{base_val:.2}")),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(0x162c1e))
                                    .border_1()
                                    .border_color(rgb(0x10b981))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(rgb(0x6ee7b7))
                                            .child("Output:"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_bold()
                                            .text_color(rgb(0x34d399))
                                            .child(format!("{evaluated_out:.2}")),
                                    ),
                            ),
                    ),
            )
            .child(
                // Row 2: graph actions (node palette lives in the sidebar).
                h_flex()
                    .w_full()
                    .h(px(30.))
                    .px_3()
                    .gap_1p5()
                    .items_center()
                    .bg(cx.theme().muted)
                    .child(div().flex_grow(1.0))
                    .child({
                        let has_copied = this_copied_link.is_some();
                        let ent_pl = ent_paste_link.clone();
                        div()
                            .id("paste_property_link_btn")
                            .test_support()
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(if has_copied { rgb(0x451a1a) } else { rgb(0x1e2029) })
                            .border_1()
                            .border_color(if has_copied { rgb(0xef4444) } else { rgb(0x3a3c4e) })
                            .hover(|s| s.bg(rgb(0x7f1d1d)).text_color(rgb(0xfecaca)))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(if has_copied { rgb(0xfca5a5) } else { rgb(0x9ca3af) })
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                cx.stop_propagation();
                                ent_pl.update(cx, |this, cx| {
                                    this.paste_copied_property_link(cx);
                                });
                            })
                            .child("Paste Property Link")
                    })
                    .child(
                        div()
                            .id("reset_graph_btn")
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(cx.theme().secondary)
                            .hover(|s| s.bg(rgb(0x7f1d1d)).text_color(rgb(0xfecaca)))
                            .border_1()
                            .border_color(cx.theme().border)
                            .cursor_pointer()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                cx.stop_propagation();
                                ent_reset.update(cx, |this, cx| {
                                    this.reset_graph(cx);
                                });
                            })
                            .child("Reset Graph"),
                    ),
            )
    }

    /// Sidebar palette sections: (category, [(button label, kind ctor)]).
    /// Each ctor builds the node's default wiring-free kind.
    type PaletteEntry = (&'static str, &'static [(&'static str, fn() -> NodeKind)]);
    const PALETTE_SECTIONS: &'static [PaletteEntry] = &[
        (
            "Value",
            &[
                ("Math", || NodeKind::Math { op: MathOp::Multiply, default_b: 1.0 }),
                ("Lerp", || NodeKind::Lerp { default_a: 0.0, default_b: 1.0 }),
                ("Clamp", || NodeKind::Clamp { min: 0.0, max: 1.0 }),
                ("Remap", || NodeKind::Remap { in_min: 0.0, in_max: 1.0, out_min: 0.0, out_max: 100.0 }),
                ("Smoothstep", || NodeKind::Smoothstep { edge0: 0.0, edge1: 1.0 }),
                ("Noise", || NodeKind::Noise { frequency: 2.0, amplitude: 50.0 }),
                ("Wave", || NodeKind::Wave { wave_type: WaveType::Sine, frequency: 1.0, amplitude: 50.0, offset: 0.0 }),
                ("Stepped", || NodeKind::Stepped { steps: 5.0 }),
            ],
        ),
        (
            "Color",
            &[
                ("Constant", || NodeKind::Constant { mode: ConstantMode::Float, value: 1.0, g: 0.0, b: 0.0, a: 1.0 }),
                ("Blend", || NodeKind::Blend { mode: GraphBlendMode::default(), default_b: 0.0 }),
                ("Posterize", || NodeKind::StockOp { plugin: StockPlugin::Posterize, params: vec![2.0] }),
                ("Threshold", || NodeKind::StockOp { plugin: StockPlugin::Threshold, params: vec![50.0, 30.0] }),
                ("Wgsl", || NodeKind::Wgsl { code: "return color;".to_string() }),
            ],
        ),
        (
            "Space",
            &[
                ("Transform", || NodeKind::Transform { x: 0.0, y: 0.0, scale: 1.0, rotation: 0.0 }),
                ("Tile", || NodeKind::Tile { repeat_x: 2.0, repeat_y: 2.0 }),
                ("Glow", || NodeKind::Glow { radius: 0.05, intensity: 1.0 }),
                ("Bloom", || NodeKind::Bloom { threshold: 0.7, radius: 0.05, intensity: 1.0 }),
            ],
        ),
        (
            "Bus",
            &[
                ("Mk 2D", || NodeKind::MakeVector { dims: 2 }),
                ("Mk 3D", || NodeKind::MakeVector { dims: 3 }),
                ("Mk Color", || NodeKind::MakeColor),
                ("Brk 2D", || NodeKind::BreakVector { dims: 2 }),
                ("Brk 3D", || NodeKind::BreakVector { dims: 3 }),
                ("Brk Color", || NodeKind::BreakColor),
            ],
        ),
        ("Driver", &[]),
    ];

    /// Vertical accordion sidebar with one collapsible section per node
    /// category. Sections start collapsed (a clean category list); a
    /// header click toggles its section open/closed.
    fn render_node_palette(&self, entity: &Entity<Self>, cx: &App) -> impl IntoElement {
        let mut sections: Vec<AnyElement> = Vec::new();
        for (i, (cat, btns)) in Self::PALETTE_SECTIONS.iter().enumerate() {
            let mut body = v_flex().px_2().py_1().gap_1p5();
            for (label, ctor) in btns.iter() {
                let ent_btn = entity.clone();
                let ctor = *ctor;
                body = body.child(self.toolbar_btn(label, ent_btn, move |this, cx| {
                    this.add_node(ctor(), cx);
                }, cx));
            }
            if *cat == "Driver" {
                let ent_drv = entity.clone();
                body = body.child(self.toolbar_btn("Driver Link", ent_drv, move |this, cx| {
                    this.add_driver_node(cx);
                }, cx));
            }
            let open = self.palette_open[i];
            let ent_hdr = entity.clone();
            let hdr = h_flex()
                .id(SharedString::from(format!("mg_pal_{i}_hdr")))
                .test_support()
                .w_full()
                .px_2()
                .py_1()
                .gap_1p5()
                .items_center()
                .cursor_pointer()
                .rounded_sm()
                .font_semibold()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .hover(|s| s.bg(cx.theme().accent))
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    ent_hdr.update(cx, |this, cx| {
                        this.palette_open[i] = !this.palette_open[i];
                        cx.notify();
                    });
                })
                .child(div().w(px(10.)).child(if open { "▾" } else { "▸" }))
                .child(cat.to_uppercase());
            let content = if open {
                body.into_any_element()
            } else {
                div().into_any_element()
            };
            let section = v_flex()
                .rounded_sm()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(hdr)
                .into_any_element();
            sections.push(
                Collapsible::new()
                    .open(open)
                    .child(section)
                    .content(content)
                    .into_any_element(),
            );
        }
        v_flex()
            .id("node_palette_sidebar")
            .w(px(150.))
            .h_full()
            .flex_shrink_0()
            .pt_1()
            .bg(cx.theme().muted)
            .border_r_1()
            .border_color(cx.theme().border)
            .overflow_y_scroll()
            .children(sections)
    }

    fn add_driver_node(&mut self, cx: &mut Context<Self>) {
        let copied = self.editor_state.read(cx).copied_property_link.clone();
        let (driver_layer_id, driver_prop_path) = match copied {
            Some((l, p)) => (l, p),
            None => (self.layer_id.clone(), "transform.rotation".to_string()),
        };
        self.add_node(
            NodeKind::DriverLink {
                driver_layer_id,
                driver_prop_path,
            },
            cx,
        );
    }

    fn toolbar_btn<F>(&self, label: &'static str, entity: Entity<Self>, on_click: F, cx: &App) -> AnyElement
    where
        F: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        div()
            .id(SharedString::from(format!("mg_add_{}", label.replace(' ', "_").to_lowercase())))
            .test_support()
            .px_2()
            .py_0p5()
            .rounded_sm()
            .bg(cx.theme().secondary)
            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
            .border_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .text_xs()
            .text_color(cx.theme().foreground)
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                cx.stop_propagation();
                entity.update(cx, |this, cx| {
                    on_click(this, cx);
                });
            })
            .child(label)
            .into_any_element()
    }

    fn render_canvas(&mut self, entity: &Entity<Self>, cx: &mut Context<Self>) -> impl IntoElement {
        let (pan_x, pan_y) = self.pan_offset;

        let ent_drag = entity.clone();
        let ent_up = entity.clone();
        let ent_pan_down = entity.clone();
        let ent_pan_up = entity.clone();
        let ent_pan_up_out = entity.clone();
        let ent_canvas_down = entity.clone();

        // Calculate positions of all sockets for wire rendering
        let mut socket_coords: HashMap<(String, String, bool), (f32, f32)> = HashMap::new();
        for node in &self.graph.nodes {
            let (nx, ny) = (node.pos_x + pan_x, node.pos_y + pan_y);
            let in_sockets = node.kind.input_sockets();
            for (idx, in_name) in in_sockets.iter().enumerate() {
                let sx = nx + 14.0;
                let sy = ny + 41.0 + (idx as f32 * 26.0);
                socket_coords.insert((node.id.clone(), (*in_name).to_string(), true), (sx, sy));
            }
            let out_sockets = node.kind.output_sockets();
            for (idx, out_name) in out_sockets.iter().enumerate() {
                let sx = nx + 186.0;
                let sy = ny + 41.0 + (idx as f32 * 26.0);
                socket_coords.insert((node.id.clone(), (*out_name).to_string(), false), (sx, sy));
            }
        }

        let mut canvas_div = div()
            .id("modifier_graph_canvas")
            .test_support()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgb(0x101116))
            // Click empty canvas cancels pending wire or deselects node
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                ent_canvas_down.update(cx, |this, cx| {
                    // Absolute canvas-space positions include the pan offset
                    // (wires and sockets are drawn panned); deltas below stay
                    // pan-free so panning never compounds into drags.
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    if let Some(wire) = &this.connecting_wire {
                        let dist = ((curr_x - wire.start_x).powi(2) + (curr_y - wire.start_y).powi(2)).sqrt();
                        if dist > 8.0 {
                            this.connecting_wire = None;
                            cx.notify();
                        }
                    } else {
                        this.selected_node = None;
                        cx.notify();
                    }
                });
            })
            // Mouse move handler for dragging nodes and wires
            .on_mouse_move(move |event, _window, cx| {
                let (curr_x, curr_y) = event_to_canvas_pos(event.position);

                ent_drag.update(cx, |this, cx| {
                    let mut changed = false;
                    // Node drag
                    if let Some((node_id, mx, my, nx, ny)) = &this.dragging_node {
                        let dx = curr_x - *mx;
                        let dy = curr_y - *my;
                        if let Some(node) = this.graph.get_node_mut(node_id) {
                            node.pos_x = *nx + dx;
                            node.pos_y = *ny + dy;
                            changed = true;
                        }
                    }
                    // Wire drag (absolute position must match the panned
                    // socket coordinates the wire renders from)
                    let (wx, wy) = this.cursor_canvas_abs(event.position);
                    if let Some(wire) = &mut this.connecting_wire {
                        wire.cur_x = wx;
                        wire.cur_y = wy;
                        changed = true;
                    }
                    // Param scrub
                    if let Some((node_id, param_name, last_x, _)) = this.scrubbing_param.clone() {
                        let dx = curr_x - last_x;
                        if dx.abs() >= 1.0 {
                            this.apply_param_scrub(&node_id, &param_name, dx * 0.1, cx);
                            this.scrub_moved = true;
                            if let Some((_, _, lx, _)) = &mut this.scrubbing_param {
                                *lx = curr_x;
                            }
                            changed = true;
                        }
                    }
                    // Canvas pan
                    if let Some((mx, my, px, py)) = &this.panning_canvas {
                        let dx = curr_x - *mx;
                        let dy = curr_y - *my;
                        this.pan_offset = (*px + dx, *py + dy);
                        changed = true;
                    }
                    if changed {
                        cx.notify();
                    }
                });
            })
            // Mouse up handler to finalize drag or drop wire
            .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
                ent_up.update(cx, |this, cx| {
                    // Snap/finish positions are pan-inclusive canvas space
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    if this.scrubbing_param.is_some() {
                        this.scrubbing_param = None;
                        this.scrub_moved = false;
                        this.sync_to_state(cx);
                    }
                    if this.dragging_node.is_some() {
                        this.dragging_node = None;
                        this.sync_to_state(cx);
                    }
                    if let Some(wire) = this.connecting_wire.clone() {
                        let (pan_x, pan_y) = this.pan_offset;
                        let connected = this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                        if !connected {
                            let dist = ((curr_x - wire.start_x).powi(2) + (curr_y - wire.start_y).powi(2)).sqrt();
                            if dist > 6.0 {
                                // Drag-dropped on empty canvas -> cancel wire
                                this.connecting_wire = None;
                                cx.notify();
                            }
                        }
                    }
                    if this.panning_canvas.is_some() {
                        this.panning_canvas = None;
                        cx.notify();
                    }
                });
            })
            // Canvas right-click for panning
            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                ent_pan_down.update(cx, |this, cx| {
                    this.panning_canvas = Some((curr_x, curr_y, this.pan_offset.0, this.pan_offset.1));
                    cx.notify();
                });
            })
            // Right-button release ends canvas panning (without this the
            // canvas stays stuck to the cursor after a right-drag pan).
            .on_mouse_up(MouseButton::Right, move |_event, _window, cx| {
                ent_pan_up.update(cx, |this, cx| {
                    if this.panning_canvas.is_some() {
                        this.panning_canvas = None;
                        cx.notify();
                    }
                });
            })
            .on_mouse_up_out(MouseButton::Right, move |_event, _window, cx| {
                ent_pan_up_out.update(cx, |this, cx| {
                    if this.panning_canvas.is_some() {
                        this.panning_canvas = None;
                        cx.notify();
                    }
                });
            });

        // 0. Paint-time canvas origin recorder: cursor handlers map through
        // the measured origin instead of a hardcoded header height (the
        // toolbar row can wrap and grow the header).
        let origin_store = self.canvas_origin.clone();
        canvas_div = canvas_div.child(
            div().absolute().size_full().child(gpui::canvas(
                move |_bounds, _window, _cx| {},
                move |bounds, _, _window, _cx| {
                    if let Ok(mut o) = origin_store.lock() {
                        *o = (bounds.origin.x / px(1.0), bounds.origin.y / px(1.0));
                    }
                },
            )),
        );

        // 3. Render Connecting Wires (beads and midpoint disconnect button)
        for conn in &self.graph.connections {
            if let (Some(&(x1, y1)), Some(&(x2, y2))) = (
                socket_coords.get(&(conn.from_node.clone(), conn.from_socket.clone(), false)),
                socket_coords.get(&(conn.to_node.clone(), conn.to_socket.clone(), true)),
            ) {
                let wire_els = self.build_bezier_wire(
                    (x1, y1),
                    (x2, y2),
                    rgb(0x38bdf8),
                    Some((conn.to_node.clone(), conn.to_socket.clone())),
                    entity,
                );
                canvas_div = canvas_div.children(wire_els);
            }
        }

        // 4. Render Active Dragging Wire (from socket to cursor)
        if let Some(wire) = &self.connecting_wire {
            if !wire.is_from_input {
                // From output socket to cursor
                if let Some(&(x1, y1)) = socket_coords.get(&(wire.node_id.clone(), wire.socket_name.clone(), false)) {
                    let wire_els = self.build_bezier_wire(
                        (x1, y1),
                        (wire.cur_x, wire.cur_y),
                        rgb(0xfbbf24), // Vibrant gold wire while connecting
                        None,
                        entity,
                    );
                    canvas_div = canvas_div.children(wire_els);
                }
            } else {
                // From cursor to input socket
                if let Some(&(x2, y2)) = socket_coords.get(&(wire.node_id.clone(), wire.socket_name.clone(), true)) {
                    let wire_els = self.build_bezier_wire(
                        (wire.cur_x, wire.cur_y),
                        (x2, y2),
                        rgb(0xfbbf24),
                        None,
                        entity,
                    );
                    canvas_div = canvas_div.children(wire_els);
                }
            }
        }

        // 5. Render Node Cards
        for node in &self.graph.nodes {
            let node_card = self.render_node_card(node, pan_x, pan_y, entity, cx);
            canvas_div = canvas_div.child(node_card);
        }

        canvas_div
    }

    /// Build one continuous stroked wire between two points (single canvas
    /// element tessellating the cubic — no bead divs), plus the optional
    /// midpoint disconnect badge overlay.
    fn build_bezier_wire(
        &self,
        from_pt: (f32, f32),
        to_pt: (f32, f32),
        bead_color: Rgba,
        disconnect_target: Option<(String, String)>,
        entity: &Entity<Self>,
    ) -> Vec<AnyElement> {
        let (x1, y1) = from_pt;
        let (x2, y2) = to_pt;
        let dx = (x2 - x1).abs().max(40.0) * 0.5;
        let p0 = (x1, y1);
        let p1 = (x1 + dx, y1);
        let p2 = (x2 - dx, y2);
        let p3 = (x2, y2);

        // Same cubic samples the bead strip used (keeps curve shape).
        let steps = 22;
        let mut pts = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let it = 1.0 - t;
            pts.push((
                it * it * it * p0.0 + 3.0 * it * it * t * p1.0 + 3.0 * it * t * t * p2.0 + t * t * t * p3.0,
                it * it * it * p0.1 + 3.0 * it * it * t * p1.1 + 3.0 * it * t * t * p2.1 + t * t * t * p3.1,
            ));
        }

        let mut elements = Vec::with_capacity(2);
        let pad = 6.0;
        let min_x = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) - pad;
        let min_y = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) - pad;
        let max_x = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) + pad;
        let max_y = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) + pad;
        let wire_id = match &disconnect_target {
            Some((to_node, to_sock)) => format!("wire_committed_{to_node}_{to_sock}"),
            None => "wire_drag".to_string(),
        };
        elements.push(
            div()
                .id(SharedString::from(wire_id))
                .test_support()
                .absolute()
                .left(px(min_x))
                .top(px(min_y))
                .w(px((max_x - min_x).max(1.0)))
                .h(px((max_y - min_y).max(1.0)))
                .child(
                    gpui::canvas(
                        move |_bounds, _window, _cx| {},
                        move |bounds, _, window, _cx| {
                            let ox = bounds.origin.x;
                            let oy = bounds.origin.y;
                            let shifted: Vec<gpui::Point<gpui::Pixels>> = pts
                                .iter()
                                .map(|p| gpui::point(px(p.0 - min_x) + ox, px(p.1 - min_y) + oy))
                                .collect();
                            if shifted.len() >= 2 {
                                let mut path = gpui::Path::new(shifted[0]);
                                for seg in shifted.windows(2) {
                                    crate::panels::draw_line_segment(&mut path, seg[0], seg[1], 2.5);
                                }
                                window.paint_path(path, bead_color);
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        );

        // Render disconnect badge at the midpoint of the wire
        if let Some((to_node, to_sock)) = disconnect_target {
            let mx = (x1 + x2) * 0.5;
            let my = (y1 + y2) * 0.5;

            let ent_disc = entity.clone();
            let tn = to_node.clone();
            let ts = to_sock.clone();

            let disconnect_btn = div()
                .id(SharedString::from(format!("disc_{}_{}", to_node, to_sock)))
                .test_support()
                .absolute()
                .left(px(mx - 8.0))
                .top(px(my - 8.0))
                .w(px(16.0))
                .h(px(16.0))
                .rounded_full()
                .bg(rgb(0x1f2937))
                .border_1()
                .border_color(rgb(0xef4444))
                .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                .cursor_pointer()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(0xf87171))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    cx.stop_propagation();
                    ent_disc.update(cx, |this, cx| {
                        this.disconnect_socket(&tn, &ts, cx);
                    });
                })
                .child("×");

            elements.push(disconnect_btn.into_any_element());
        }

        elements
    }

    /// Render an individual node card with its sockets and inline parameters.
    fn render_node_card(
        &self,
        node: &ModifierNode,
        pan_x: f32,
        pan_y: f32,
        entity: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let node_id = node.id.clone();
        let (nx, ny) = (node.pos_x + pan_x, node.pos_y + pan_y);
        let is_selected = self.selected_node.as_deref() == Some(&node_id);

        let (banner_bg, banner_color) = match node.kind {
            NodeKind::GetLayerFactor | NodeKind::BaseValueIn | NodeKind::Constant { .. } => {
                (rgb(0x1e3a8a), rgb(0x93c5fd)) // Deep blue
            }
            NodeKind::MakeVector { .. } | NodeKind::MakeColor | NodeKind::BreakVector { .. } | NodeKind::BreakColor => {
                (rgb(0x0f766e), rgb(0x5eead4)) // Teal for vector/color buses
            }
            NodeKind::Math { .. } | NodeKind::Lerp { .. } | NodeKind::Clamp { .. } | NodeKind::Remap { .. } | NodeKind::Smoothstep { .. } => {
                (rgb(0x581c87), rgb(0xd8b4fe)) // Deep purple
            }
            NodeKind::Noise { .. } | NodeKind::Wave { .. } | NodeKind::Stepped { .. } => {
                (rgb(0x14532d), rgb(0x86efac)) // Deep green
            }
            NodeKind::Blend { .. } => {
                (rgb(0x312e81), rgb(0xc7d2fe)) // Indigo
            }
            NodeKind::StockOp { .. } => {
                (rgb(0x701a75), rgb(0xf0abfc)) // Deep magenta
            }
            NodeKind::Wgsl { .. } => {
                (rgb(0x312e81), rgb(0xa5b4fc)) // Indigo-slate for shader nodes
            }
            NodeKind::Transform { .. } | NodeKind::Tile { .. } => {
                (rgb(0x134e4a), rgb(0x5eead4)) // Teal for spatial layout nodes
            }
            NodeKind::Glow { .. } | NodeKind::Bloom { .. } => {
                (rgb(0x713f12), rgb(0xfde047)) // Amber for light nodes
            }
            NodeKind::DriverLink { .. } => {
                (rgb(0x7f1d1d), rgb(0xfca5a5)) // Deep red for driver links
            }
            NodeKind::Output => {
                (rgb(0x78350f), rgb(0xfde68a)) // Deep amber
            }
        };

        let ent_drag = entity.clone();
        let ent_del = entity.clone();
        let ent_card_click = entity.clone();
        let ent_card_up = entity.clone();
        let nid_drag = node_id.clone();
        let nid_del = node_id.clone();
        let nid_card = node_id.clone();
        let npos_x = node.pos_x;
        let npos_y = node.pos_y;

        let mut card = div()
            .id(SharedString::from(format!("node_card_{}", node.id)))
            .absolute()
            .left(px(nx))
            .top(px(ny))
            .w(px(200.0))
            .bg(rgb(0x1a1b23))
            .rounded_md()
            .border_1()
            .border_color(if is_selected { rgb(0x38bdf8) } else { rgb(0x2e303e) })
            .shadow_md()
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                cx.stop_propagation();
                ent_card_click.update(cx, |this, cx| {
                    this.selected_node = Some(nid_card.clone());
                    cx.notify();
                });
            })
            .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                ent_card_up.update(cx, |this, cx| {
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    if this.dragging_node.is_some() {
                        this.dragging_node = None;
                        this.sync_to_state(cx);
                    }
                    if this.connecting_wire.is_some() {
                        let (pan_x, pan_y) = this.pan_offset;
                        this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                    }
                });
            });

        // 1. Header with title, drag handler, and delete button
        let is_output = matches!(node.kind, NodeKind::Output);
        // Effect-graph windows read the incoming node as "Content".
        let header_title = if matches!(node.kind, NodeKind::BaseValueIn) && self.is_effect_graph {
            "Content".to_string()
        } else {
            node.title.clone()
        };
        let header = h_flex()
            .w_full()
            .h(px(26.0))
            .px_2()
            .rounded_t_md()
            .bg(banner_bg)
            .justify_between()
            .items_center()
            .cursor_move()
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                ent_drag.update(cx, |this, cx| {
                    this.selected_node = Some(nid_drag.clone());
                    this.dragging_node = Some((nid_drag.clone(), curr_x, curr_y, npos_x, npos_y));
                    cx.notify();
                });
            })
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(banner_color)
                    .child(header_title),
            )
            .child(if !is_output {
                div()
                    .id(SharedString::from(format!("del_node_{}", node.id)))
                    .cursor_pointer()
                    .text_xs()
                    .text_color(rgb(0x9ca3af))
                    .hover(|s| s.text_color(rgb(0xef4444)))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        cx.stop_propagation();
                        ent_del.update(cx, |this, cx| {
                            this.remove_node(&nid_del, cx);
                        });
                    })
                    .child("×")
            } else {
                div()
                    .id(SharedString::from(format!("output_star_{}", node.id)))
                    .text_xs()
                    .text_color(banner_color)
                    .child("★")
            });

        card = card.child(header);

        // 2. Sockets Area (Input on left, Output on right)
        let in_sockets = node.kind.input_sockets();
        let out_sockets = node.kind.output_sockets();

        let max_sockets = in_sockets.len().max(out_sockets.len());
        if max_sockets > 0 {
            let mut sockets_col = v_flex().w_full().py_1().px_2().gap_1();

            for i in 0..max_sockets {
                let in_opt = in_sockets.get(i);
                let out_opt = out_sockets.get(i);

                let in_widget = if let Some(in_name) = in_opt {
                    self.render_input_socket(&node_id, in_name, entity, cx)
                } else {
                    div()
                };

                let out_widget = if let Some(out_name) = out_opt {
                    self.render_output_socket(&node_id, out_name, entity, cx)
                } else {
                    div()
                };

                sockets_col = sockets_col.child(
                    h_flex()
                        .w_full()
                        .h(px(22.0))
                        .justify_between()
                        .items_center()
                        .child(in_widget)
                        .child(out_widget),
                );
            }

            card = card.child(sockets_col);
        }

        // 3. Inline Parameter Controls for this node kind
        let params_widget = self.render_node_params(node, entity, cx);
        card = card.child(params_widget);

        card
    }

    /// Render an input socket on a node card.
    fn render_input_socket(
        &self,
        node_id: &str,
        socket_name: &'static str,
        entity: &Entity<Self>,
        _cx: &mut Context<Self>,
    ) -> Div {
        let nid = node_id.to_string();
        let sname = socket_name.to_string();

        let is_connected = self
            .graph
            .connections
            .iter()
            .any(|c| c.to_node == nid && c.to_socket == sname);

        let is_target_hover = self.connecting_wire.as_ref().is_some_and(|w| {
            !w.is_from_input && w.node_id != nid
        });

        let ent_down = entity.clone();
        let ent_up = entity.clone();
        let ent_rclick = entity.clone();
        let nid_down = nid.clone();
        let nid_rclick = nid.clone();
        let sname_down = sname.clone();
        let sname_rclick = sname.clone();

        let label = socket_display_label(
            self.graph.get_node(node_id).map(|n| &n.kind),
            socket_name,
        );

        h_flex()
            .gap_1p5()
            .items_center()
            .cursor_pointer()
            .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                ent_up.update(cx, |this, cx| {
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    let (pan_x, pan_y) = this.pan_offset;
                    this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                });
            })
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                ent_down.update(cx, |this, cx| {
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    let (pan_x, pan_y) = this.pan_offset;
                    if this.connecting_wire.is_some() {
                        this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                    } else {
                        // If already connected, disconnect and pick up wire to reconnect!
                        if let Some(existing) = this.graph.connections.iter().find(|c| c.to_node == nid_down && c.to_socket == sname_down).cloned() {
                            this.disconnect_socket(&nid_down, &sname_down, cx);
                            this.connecting_wire = Some(WireDragState {
                                is_from_input: false,
                                node_id: existing.from_node,
                                socket_name: existing.from_socket,
                                cur_x: curr_x,
                                cur_y: curr_y,
                                start_x: curr_x,
                                start_y: curr_y,
                            });
                        } else {
                            // Start dragging from input socket to an output
                            this.connecting_wire = Some(WireDragState {
                                is_from_input: true,
                                node_id: nid_down.clone(),
                                socket_name: sname_down.clone(),
                                cur_x: curr_x,
                                cur_y: curr_y,
                                start_x: curr_x,
                                start_y: curr_y,
                            });
                        }
                        cx.notify();
                    }
                });
            })
            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                cx.stop_propagation();
                ent_rclick.update(cx, |this, cx| {
                    this.disconnect_socket(&nid_rclick, &sname_rclick, cx);
                });
            })
            .child(
                div()
                    .id(SharedString::from(format!("in_{}_{}", node_id, socket_name)))
                    .test_support()
                    .w(px(12.0))
                    .h(px(12.0))
                    .rounded_full()
                    .bg(if is_connected {
                        rgb(0x38bdf8)
                    } else if is_target_hover {
                        rgb(0x0284c7)
                    } else {
                        rgb(0x4b5563)
                    })
                    .border_1()
                    .border_color(if is_target_hover {
                        rgb(0x38bdf8)
                    } else {
                        rgb(0x9ca3af)
                    })
                    .hover(|s| s.bg(rgb(0x0ea5e9)).border_color(rgb(0xffffff))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if is_target_hover { rgb(0x38bdf8) } else { rgb(0x9ca3af) })
                    .child(label),
            )
    }

    /// Render an output socket on a node card.
    fn render_output_socket(
        &self,
        node_id: &str,
        socket_name: &'static str,
        entity: &Entity<Self>,
        _cx: &mut Context<Self>,
    ) -> Div {
        let nid = node_id.to_string();
        let sname = socket_name.to_string();

        let is_connected = self
            .graph
            .connections
            .iter()
            .any(|c| c.from_node == nid && c.from_socket == sname);

        let is_target_hover = self.connecting_wire.as_ref().is_some_and(|w| {
            w.is_from_input && w.node_id != nid
        });

        let ent_down = entity.clone();
        let ent_up = entity.clone();
        let ent_rclick = entity.clone();
        let nid_down = nid.clone();
        let nid_rclick = nid.clone();
        let sname_down = sname.clone();
        let sname_rclick = sname.clone();

        let out_label = socket_display_label(
            self.graph.get_node(node_id).map(|n| &n.kind),
            socket_name,
        );

        h_flex()
            .gap_1p5()
            .items_center()
            .cursor_pointer()
            .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                ent_up.update(cx, |this, cx| {
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    let (pan_x, pan_y) = this.pan_offset;
                    this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                });
            })
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                ent_down.update(cx, |this, cx| {
                    let (curr_x, curr_y) = this.cursor_canvas_abs(event.position);
                    let (pan_x, pan_y) = this.pan_offset;
                    if this.connecting_wire.is_some() {
                        this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                    } else {
                        this.connecting_wire = Some(WireDragState {
                            is_from_input: false,
                            node_id: nid_down.clone(),
                            socket_name: sname_down.clone(),
                            cur_x: curr_x,
                            cur_y: curr_y,
                            start_x: curr_x,
                            start_y: curr_y,
                        });
                        cx.notify();
                    }
                });
            })
            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                cx.stop_propagation();
                ent_rclick.update(cx, |this, cx| {
                    this.graph.connections.retain(|c| !(c.from_node == nid_rclick && c.from_socket == sname_rclick));
                    this.sync_to_state(cx);
                });
            })
            .child(
                div()
                    .text_xs()
                    .text_color(if is_target_hover { rgb(0x34d399) } else { rgb(0x9ca3af) })
                    .child(out_label),
            )
            .child(
                div()
                    .id(SharedString::from(format!("out_{}_{}", node_id, socket_name)))
                    .test_support()
                    .w(px(12.0))
                    .h(px(12.0))
                    .rounded_full()
                    .bg(if is_connected {
                        rgb(0x10b981)
                    } else if is_target_hover {
                        rgb(0x059669)
                    } else {
                        rgb(0x4b5563)
                    })
                    .border_1()
                    .border_color(if is_target_hover {
                        rgb(0x34d399)
                    } else {
                        rgb(0x9ca3af)
                    })
                    .hover(|s| s.bg(rgb(0x059669)).border_color(rgb(0xffffff))),
            )
    }

    /// Render inline interactive parameters inside a node card.
    fn render_node_params(
        &self,
        node: &ModifierNode,
        entity: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Div {
        let nid = node.id.clone();
        let ent = entity.clone();

        match &node.kind {
            NodeKind::Constant { mode, value, g, b, a } => {
                let m = *mode;
                let (v, gv, bv, av) = (*value, *g, *b, *a);
                let mut col = v_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .border_t_1()
                    .border_color(rgb(0x2e303e));

                // Mode picker: Float / Int / Color / 2D / 3D
                let mut pills = h_flex().w_full().gap_1();
                pills = pills
                    .child(self.const_mode_btn("Float", m == ConstantMode::Float, ent.clone(), nid.clone(), ConstantMode::Float, cx))
                    .child(self.const_mode_btn("Int", m == ConstantMode::Int, ent.clone(), nid.clone(), ConstantMode::Int, cx))
                    .child(self.const_mode_btn("Color", m == ConstantMode::Color, ent.clone(), nid.clone(), ConstantMode::Color, cx))
                    .child(self.const_mode_btn("V2", m == ConstantMode::Vec2, ent.clone(), nid.clone(), ConstantMode::Vec2, cx))
                    .child(self.const_mode_btn("V3", m == ConstantMode::Vec3, ent.clone(), nid.clone(), ConstantMode::Vec3, cx));
                col = col.child(pills);

                let row = |this: &Self, label: &'static str, pname: &'static str, val: f32, tint: u32| {
                    h_flex()
                        .w_full()
                        .gap_1p5()
                        .items_center()
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(label))
                        .child(this.param_scrub_input(nid.clone(), pname, val, rgb(tint), ent.clone(), cx))
                };
                match m {
                    ConstantMode::Float | ConstantMode::Int => {
                        col = col.child(row(self, "Value:", "value", v, 0x38bdf8));
                    }
                    ConstantMode::Vec2 => {
                        col = col.child(row(self, "X:", "value", v, 0x38bdf8));
                        col = col.child(row(self, "Y:", "g", gv, 0x4ade80));
                    }
                    ConstantMode::Vec3 => {
                        col = col.child(row(self, "X:", "value", v, 0x38bdf8));
                        col = col.child(row(self, "Y:", "g", gv, 0x4ade80));
                        col = col.child(row(self, "Z:", "b", bv, 0xfbbf24));
                    }
                    ConstantMode::Color => {
                        col = col
                            .child(row(self, "R:", "value", v, 0xf87171))
                            .child(row(self, "G:", "g", gv, 0x4ade80))
                            .child(row(self, "B:", "b", bv, 0x60a5fa))
                            .child(row(self, "A:", "a", av, 0xe5e7eb));
                    }
                }
                col
            }

            NodeKind::Math { op, default_b } => {
                let current_op = *op;
                let def_b = *default_b;

                v_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .child(
                        // Operation picker buttons
                        h_flex()
                            .w_full()
                            .gap_1()
                            .justify_between()
                            .child(self.op_btn("+", current_op == MathOp::Add, ent.clone(), nid.clone(), MathOp::Add, cx))
                            .child(self.op_btn("−", current_op == MathOp::Subtract, ent.clone(), nid.clone(), MathOp::Subtract, cx))
                            .child(self.op_btn("×", current_op == MathOp::Multiply, ent.clone(), nid.clone(), MathOp::Multiply, cx))
                            .child(self.op_btn("÷", current_op == MathOp::Divide, ent.clone(), nid.clone(), MathOp::Divide, cx))
                            .child(self.op_btn("^", current_op == MathOp::Power, ent.clone(), nid.clone(), MathOp::Power, cx)),
                    )
                    .child(
                        // Fallback B value
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Default B:"))
                            .child(self.param_scrub_input(nid, "default_b", def_b, rgb(0xc084fc), ent, cx)),
                    )
            }

            NodeKind::Wave { wave_type, frequency, amplitude, .. } => {
                let wt = *wave_type;
                let freq = *frequency;
                let amp = *amplitude;
                let nid_wt1 = nid.clone();
                let nid_wt2 = nid.clone();
                let nid_wt3 = nid.clone();
                let ent_wt1 = ent.clone();
                let ent_wt2 = ent.clone();
                let ent_wt3 = ent.clone();

                v_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .justify_between()
                            .child(self.wave_btn("Sine", wt == WaveType::Sine, ent_wt1, nid_wt1, WaveType::Sine, cx))
                            .child(self.wave_btn("Tri", wt == WaveType::Triangle, ent_wt2, nid_wt2, WaveType::Triangle, cx))
                            .child(self.wave_btn("Sqr", wt == WaveType::Square, ent_wt3, nid_wt3, WaveType::Square, cx)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Freq:"))
                            .child(self.param_scrub_input(nid, "frequency", freq, rgb(0x4ade80), ent, cx)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Amp: {amp:.1}")),
                    )
            }

            NodeKind::Stepped { steps } => {
                let st = *steps;

                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Steps:"))
                    .child(self.param_scrub_input(nid, "steps", st, rgb(0x4ade80), ent, cx))
            }

            NodeKind::Blend { mode, default_b } => {
                let m = *mode;
                let def_b = *default_b;
                let combo_id = nid.replace('-', "_");
                v_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .child(
                        div()
                            .id(SharedString::from(format!("mg_blend_combo_{combo_id}")))
                            .test_support()
                            .w_full()
                            .child(match self.blend_combos.get(&nid) {
                                Some(cb) => Combobox::new(cb)
                                    .menu_width(px(150.))
                                    .into_any_element(),
                                None => div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(m.label())
                                    .into_any_element(),
                            }),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Default fg:"))
                            .child(self.param_scrub_input(nid, "default_b", def_b, rgb(0xc084fc), ent, cx)),
                    )
            }

            NodeKind::Wgsl { code } => self.render_wgsl_params(&nid, code, entity, cx),

            NodeKind::Transform { x, y, scale, rotation } => self.spatial_rows(
                vec![
                    ("X:", "x", *x, 0x38bdf8),
                    ("Y:", "y", *y, 0x4ade80),
                    ("Scale:", "scale", *scale, 0xfbbf24),
                    ("Rotation:", "rotation", *rotation, 0xf87171),
                ],
                nid,
                ent,
                cx,
            ),
            NodeKind::Tile { repeat_x, repeat_y } => self.spatial_rows(
                vec![
                    ("Repeat X:", "repeat_x", *repeat_x, 0x38bdf8),
                    ("Repeat Y:", "repeat_y", *repeat_y, 0x4ade80),
                ],
                nid,
                ent,
                cx,
            ),
            NodeKind::Glow { radius, intensity } => self.spatial_rows(
                vec![
                    ("Radius:", "radius", *radius, 0xfbbf24),
                    ("Intensity:", "intensity", *intensity, 0xf87171),
                ],
                nid,
                ent,
                cx,
            ),
            NodeKind::Bloom { threshold, radius, intensity } => self.spatial_rows(
                vec![
                    ("Threshold:", "threshold", *threshold, 0xe5e7eb),
                    ("Radius:", "radius", *radius, 0xfbbf24),
                    ("Intensity:", "intensity", *intensity, 0xf87171),
                ],
                nid,
                ent,
                cx,
            ),

            NodeKind::DriverLink { driver_layer_id, driver_prop_path } => {
                let dl = driver_layer_id.clone();
                let dp = driver_prop_path.clone();
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .w(px(14.))
                            .h(px(14.))
                            .items_center()
                            .justify_center()
                            .text_color(rgb(0xef4444))
                            .child(IconName::Link),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .text_color(rgb(0xf87171))
                            .child(format!("{} · {}", dl, dp)),
                    )
            }

            _ => div(),
        }
    }

    fn op_btn(
        &self,
        symbol: &'static str,
        active: bool,
        entity: Entity<Self>,
        node_id: String,
        target_op: MathOp,
        _cx: &App,
    ) -> Div {
        div()
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .bg(if active { rgb(0x7e22ce) } else { rgb(0x232430) })
            .hover(|s| s.bg(rgb(0x9333ea)).text_color(rgb(0xffffff)))
            .cursor_pointer()
            .text_xs()
            .font_bold()
            .text_color(if active { rgb(0xffffff) } else { rgb(0x9ca3af) })
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                cx.stop_propagation();
                let nid = node_id.clone();
                entity.update(cx, |this, cx| {
                    if let Some(n) = this.graph.get_node_mut(&nid) {
                        if let NodeKind::Math { op, .. } = &mut n.kind {
                            *op = target_op;
                            this.sync_to_state(cx);
                        }
                    }
                });
            })
            .child(symbol)
    }

    /// Shared scrub-row stack for the spatial nodes (Transform/Tile/Glow/
    /// Bloom): label + param pill per row.
    fn spatial_rows(
        &self,
        rows: Vec<(&'static str, &'static str, f32, u32)>,
        nid: String,
        ent: Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut col = v_flex()
            .w_full()
            .px_2()
            .py_1p5()
            .gap_1()
            .border_t_1()
            .border_color(rgb(0x2e303e));
        for (label, pname, val, tint) in rows {
            col = col.child(
                h_flex()
                    .w_full()
                    .gap_1p5()
                    .items_center()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(label))
                    .child(self.param_scrub_input(nid.clone(), pname, val, rgb(tint), ent.clone(), cx)),
            );
        }
        col
    }

    /// Wgsl node body: click-to-edit code pill + the shader signature hint.
    fn render_wgsl_params(
        &self,
        node_id: &str,
        code: &str,
        entity: &Entity<Self>,
        _cx: &mut Context<Self>,
    ) -> Div {
        let ent_open = entity.clone();
        let is_editing = self
            .editing_param
            .as_ref()
            .is_some_and(|(nid, pn)| nid == node_id && pn == "code");
        if is_editing {
            if let Some(editor) = self.param_editor.clone() {
                let ent_esc = entity.clone();
                return v_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .on_action(move |_: &Escape, _window, cx| {
                        ent_esc.update(cx, |this, cx| this.cancel_param_edit(cx));
                    })
                    .child(Input::new(&editor).w_full());
            }
        }
        let line = code.lines().next().unwrap_or("");
        let truncated: String = if line.chars().count() > 34 {
            format!("{}…", line.chars().take(34).collect::<String>())
        } else {
            line.to_string()
        };
        let display = if truncated.trim().is_empty() {
            "return color;".to_string()
        } else {
            truncated
        };
        let nid_open = node_id.to_string();
        v_flex()
            .w_full()
            .px_2()
            .py_1p5()
            .gap_1()
            .border_t_1()
            .border_color(rgb(0x2e303e))
            .child(
                div()
                    .id(SharedString::from(format!("mg_wgsl_code_{node_id}")))
                    .test_support()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(0x232430))
                    .border_1()
                    .border_color(rgb(0x3a3c4e))
                    .hover(|s| s.border_color(rgb(0x8b5cf6)))
                    .cursor_pointer()
                    .text_xs()
                    .text_color(rgb(0x93c5fd))
                    .child(display)
                    .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
                        cx.stop_propagation();
                        let nid = nid_open.clone();
                        ent_open.update(cx, |this, cx| {
                            this.begin_wgsl_code_edit(&nid, window, cx)
                        });
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x6b7280))
                    .child("fn(color, uv, p) -> vec4 — WGSL runs on the GPU (CPU passes through)"),
            )
    }

    /// Mode pill for a Constant node — swaps the component layout in place
    /// (wires survive because the node id never changes).
    fn const_mode_btn(
        &self,
        label: &'static str,
        active: bool,
        entity: Entity<Self>,
        node_id: String,
        target_mode: ConstantMode,
        _cx: &App,
    ) -> AnyElement {
        div()
            .id(SharedString::from(format!(
                "{}_{}",
                node_id.replace('-', "_"),
                label.to_lowercase()
            )))
            .test_support()
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .bg(if active { rgb(0x38bdf8) } else { rgb(0x232430) })
            .hover(|s| s.bg(rgb(0x0ea5e9)).text_color(rgb(0xffffff)))
            .cursor_pointer()
            .text_xs()
            .font_bold()
            .text_color(if active { rgb(0x0b1120) } else { rgb(0x9ca3af) })
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                cx.stop_propagation();
                let nid = node_id.clone();
                entity.update(cx, |this, cx| {
                    if let Some(n) = this.graph.get_node_mut(&nid) {
                        if let NodeKind::Constant { mode, value: _, g: _, b: _, a } = &mut n.kind {
                            if *mode != target_mode && target_mode == ConstantMode::Color && *a == 0.0 {
                                // Entering color mode without an alpha yet:
                                // default opaque so the constant isn't invisible.
                                *a = 1.0;
                            }
                            *mode = target_mode;
                            this.sync_to_state(cx);
                        }
                    }
                });
            })
            .child(label)
            .into_any_element()
    }

    fn wave_btn(
        &self,
        label: &'static str,
        active: bool,
        entity: Entity<Self>,
        node_id: String,
        target_wt: WaveType,
        _cx: &App,
    ) -> Div {
        div()
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .bg(if active { rgb(0x15803d) } else { rgb(0x232430) })
            .hover(|s| s.bg(rgb(0x16a34a)).text_color(rgb(0xffffff)))
            .cursor_pointer()
            .text_xs()
            .font_semibold()
            .text_color(if active { rgb(0xffffff) } else { rgb(0x9ca3af) })
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                cx.stop_propagation();
                let nid = node_id.clone();
                entity.update(cx, |this, cx| {
                    if let Some(n) = this.graph.get_node_mut(&nid) {
                        if let NodeKind::Wave { wave_type, .. } = &mut n.kind {
                            *wave_type = target_wt;
                            this.sync_to_state(cx);
                        }
                    }
                });
            })
            .child(label)
    }

    fn param_scrub_input(
        &self,
        node_id: String,
        param_name: &'static str,
        value: f32,
        val_color: Rgba,
        entity: Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let nid_scrub = node_id.clone();
        let pname_scrub = param_name.to_string();
        let ent_scrub = entity.clone();
        let nid_up = node_id.clone();
        let pname_up = param_name.to_string();
        let ent_scrub_up = entity.clone();

        // Click (no drag) opens keyboard entry: render the live editor
        // instead of the value label, like timeline value pills.
        let is_editing = self
            .editing_param
            .as_ref()
            .is_some_and(|(nid, pn)| nid == &node_id && pn == param_name);
        if is_editing {
            if let Some(editor) = self.param_editor.clone() {
                let ent_esc = entity.clone();
                return div()
                    .id(SharedString::from(format!("mg_param_editor_{node_id}_{param_name}")))
                    .test_support()
                    .w(px(72.))
                    .on_action(move |_: &Escape, _window, cx| {
                        ent_esc.update(cx, |this, cx| {
                            this.cancel_param_edit(cx);
                        });
                    })
                    .child(Input::new(&editor).w_full())
                    .into_any_element();
            }
        }

        div()
            .id(SharedString::from(format!("mg_param_{node_id}_{param_name}")))
            .test_support()
            .px_2()
            .py_0p5()
            .rounded_sm()
            .bg(rgb(0x232430))
            .border_1()
            .border_color(rgb(0x3a3c4e))
            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
            .cursor_col_resize()
            .text_xs()
            .font_medium()
            .text_color(val_color)
            .child(format!("{value:.2}"))
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                cx.stop_propagation();
                let curr_x = event.position.x / px(1.0);
                let nid = nid_scrub.clone();
                let pname = pname_scrub.clone();
                ent_scrub.update(cx, |this, cx| {
                    // Committing a half-typed edit first keeps typed text
                    // from being silently lost when scrubbing elsewhere.
                    this.commit_open_param_edit(cx);
                    this.scrubbing_param = Some((nid, pname, curr_x, value));
                    this.scrub_moved = false;
                    cx.notify();
                });
            })
            // Release without movement is a click, not a scrub: open
            // keyboard entry (handled on the pill itself so it never
            // depends on canvas bubbling).
            .on_mouse_up(MouseButton::Left, move |_event, window, cx| {
                cx.stop_propagation();
                let nid = nid_up.clone();
                let pname = pname_up.clone();
                ent_scrub_up.update(cx, |this, cx| {
                    let clicked = matches!(&this.scrubbing_param, Some((n, p, _, _)) if n == &nid && p == &pname)
                        && !this.scrub_moved;
                    if clicked {
                        this.scrubbing_param = None;
                        this.scrub_moved = false;
                        this.begin_param_value_edit(&nid, &pname, window, cx);
                    }
                });
            })
            .into_any_element()
    }

    /// Open keyboard entry for a node param, prefilled with its current
    /// value (mirrors the timeline value-pill editor).
    fn begin_param_value_edit(
        &mut self,
        node_id: &str,
        param_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = self
            .graph
            .get_node(node_id)
            .map(|n| match (&n.kind, param_name) {
                (NodeKind::Constant { value, .. }, "value") => *value,
                (NodeKind::Constant { g, .. }, "g") => *g,
                (NodeKind::Constant { b, .. }, "b") => *b,
                (NodeKind::Constant { a, .. }, "a") => *a,
                (NodeKind::Math { default_b, .. }, "default_b") => *default_b,
                (NodeKind::Wave { frequency, .. }, "frequency") => *frequency,
                (NodeKind::Stepped { steps }, "steps") => *steps,
                _ => 0.0,
            })
            .unwrap_or(0.0);
        let text = if (initial - initial.round()).abs() < 1e-4 {
            format!("{}", initial.round() as i64)
        } else {
            format!("{initial:.2}")
        };
        let editor = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(text, window, cx);
            st.select_all(window, cx);
            st
        });
        let nid = node_id.to_string();
        let pname = param_name.to_string();
        let sub = cx.subscribe(
            &editor,
            move |this: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
                match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.commit_open_param_edit(cx);
                        let _ = input;
                    }
                    _ => {}
                }
            },
        );
        let handle = editor.read(cx).focus_handle(cx);
        self.editing_param = Some((nid, pname));
        self.param_editor = Some(editor);
        self.param_editor_sub = Some(sub);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Open keyboard entry for a Wgsl node's raw code (same editor
    /// machinery as param values; the commit path stores the string).
    fn begin_wgsl_code_edit(
        &mut self,
        node_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = self
            .graph
            .get_node(node_id)
            .and_then(|n| match &n.kind {
                NodeKind::Wgsl { code } => Some(code.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "return color;".to_string());
        let editor = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(initial, window, cx);
            st.select_all(window, cx);
            st
        });
        let nid = node_id.to_string();
        let sub = cx.subscribe(
            &editor,
            move |this: &mut Self, _input: Entity<InputState>, event: &InputEvent, cx| {
                match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.commit_open_param_edit(cx);
                    }
                    _ => {}
                }
            },
        );
        let handle = editor.read(cx).focus_handle(cx);
        self.editing_param = Some((nid, "code".to_string()));
        self.param_editor = Some(editor);
        self.param_editor_sub = Some(sub);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Commit the open editor text as an absolute param value (Enter/Blur
    /// or starting to scrub elsewhere). Unparseable text keeps the old value.
    /// The `code` param commits the raw string into the Wgsl node instead.
    fn commit_open_param_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.param_editor.clone() {
            let text = editor.read(cx).value().trim().to_string();
            if let Some((nid, pname)) = self.editing_param.clone() {
                if pname == "code" {
                    if let Some(n) = self.graph.get_node_mut(&nid) {
                        if let NodeKind::Wgsl { code } = &mut n.kind {
                            *code = text.clone();
                        }
                    }
                    self.sync_to_state(cx);
                }
            }
            if let (Ok(v), Some((nid, pname))) = (text.parse::<f32>(), self.editing_param.clone()) {
                self.apply_param_value(&nid, &pname, v, cx);
            }
        }
        self.editing_param = None;
        self.param_editor = None;
        self.param_editor_sub = None;
        cx.notify();
    }

    /// Close keyboard entry without further changes.
    fn cancel_param_edit(&mut self, cx: &mut Context<Self>) {
        self.editing_param = None;
        self.param_editor = None;
        self.param_editor_sub = None;
        cx.notify();
    }

    /// Set a node param to an absolute value (typed entry). Mirrors the
    /// per-param clamps in [`Self::apply_param_scrub`].
    fn apply_param_value(
        &mut self,
        node_id: &str,
        param_name: &str,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        if !value.is_finite() {
            return;
        }
        if let Some(n) = self.graph.get_node_mut(node_id) {
            match (&mut n.kind, param_name) {
                (NodeKind::Constant { value: v, .. }, "value") => {
                    *v = value;
                }
                (NodeKind::Constant { g, .. }, "g") => {
                    *g = value;
                }
                (NodeKind::Constant { b, .. }, "b") => {
                    *b = value;
                }
                (NodeKind::Constant { a, .. }, "a") => {
                    *a = value;
                }
                (NodeKind::Math { default_b, .. }, "default_b") => {
                    *default_b = value;
                }
                (NodeKind::Wave { frequency, .. }, "frequency") => {
                    *frequency = value.max(0.01);
                }
                (NodeKind::Stepped { steps }, "steps") => {
                    *steps = value.round().max(1.0);
                }
                (NodeKind::Transform { x, .. }, "x") => *x = value,
                (NodeKind::Transform { y, .. }, "y") => *y = value,
                (NodeKind::Transform { scale, .. }, "scale") => *scale = value.max(0.01),
                (NodeKind::Transform { rotation, .. }, "rotation") => *rotation = value,
                (NodeKind::Tile { repeat_x, .. }, "repeat_x") => *repeat_x = value.max(0.1),
                (NodeKind::Tile { repeat_y, .. }, "repeat_y") => *repeat_y = value.max(0.1),
                (NodeKind::Glow { radius, .. }, "radius") => *radius = value.max(0.0),
                (NodeKind::Glow { intensity, .. }, "intensity") => *intensity = value.max(0.0),
                (NodeKind::Bloom { threshold, .. }, "threshold") => *threshold = value.clamp(0.0, 1.0),
                (NodeKind::Bloom { radius, .. }, "radius") => *radius = value.max(0.0),
                (NodeKind::Bloom { intensity, .. }, "intensity") => *intensity = value.max(0.0),
                _ => return,
            }
            self.sync_to_state(cx);
        }
    }

    fn apply_param_scrub(&mut self, node_id: &str, param_name: &str, delta: f32, cx: &mut Context<Self>) {
        if let Some(n) = self.graph.get_node_mut(node_id) {
            match (&mut n.kind, param_name) {
                (NodeKind::Constant { value, .. }, "value") => {
                    *value += delta;
                }
                (NodeKind::Constant { g, .. }, "g") => {
                    *g += delta;
                }
                (NodeKind::Constant { b, .. }, "b") => {
                    *b += delta;
                }
                (NodeKind::Constant { a, .. }, "a") => {
                    *a += delta;
                }
                (NodeKind::Math { default_b, .. }, "default_b") => {
                    *default_b += delta;
                }
                (NodeKind::Wave { frequency, .. }, "frequency") => {
                    *frequency = (*frequency + delta * 0.1).max(0.01);
                }
                (NodeKind::Stepped { steps }, "steps") => {
                    *steps = (*steps + delta * 0.2).round().max(1.0);
                }
                (NodeKind::Transform { x, .. }, "x") => *x += delta,
                (NodeKind::Transform { y, .. }, "y") => *y += delta,
                (NodeKind::Transform { scale, .. }, "scale") => {
                    *scale = (*scale + delta * 0.05).max(0.01);
                }
                (NodeKind::Transform { rotation, .. }, "rotation") => *rotation += delta,
                (NodeKind::Tile { repeat_x, .. }, "repeat_x") => {
                    *repeat_x = (*repeat_x + delta * 0.05).max(0.1);
                }
                (NodeKind::Tile { repeat_y, .. }, "repeat_y") => {
                    *repeat_y = (*repeat_y + delta * 0.05).max(0.1);
                }
                (NodeKind::Glow { radius, .. }, "radius") => {
                    *radius = (*radius + delta * 0.002).max(0.0);
                }
                (NodeKind::Glow { intensity, .. }, "intensity") => {
                    *intensity = (*intensity + delta * 0.02).max(0.0);
                }
                (NodeKind::Bloom { threshold, .. }, "threshold") => {
                    *threshold = (*threshold + delta * 0.01).clamp(0.0, 1.0);
                }
                (NodeKind::Bloom { radius, .. }, "radius") => {
                    *radius = (*radius + delta * 0.002).max(0.0);
                }
                (NodeKind::Bloom { intensity, .. }, "intensity") => {
                    *intensity = (*intensity + delta * 0.02).max(0.0);
                }
                _ => {}
            }
            self.sync_to_state(cx);
        }
    }
}
