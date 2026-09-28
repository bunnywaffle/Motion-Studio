use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex, StyledExt, TestSupportExt};
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
use gpui_kit::*;
use project::modifier::{MathOp, ModifierGraph, ModifierNode, NodeKind, WaveType};

use crate::state::EditorState;

/// Total pixel height of the top header area (Row 1: 42px + Row 2: 34px + border: 1px = 77px).
pub const HEADER_HEIGHT: f32 = 77.0;

/// Map window-space cursor coordinates to canvas-space coordinates.
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
    let title = format!("Modifier Graph — {} (Layer {})", prop_path, layer_id);
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
    _subscription: Subscription,
    /// Dragging a node: `(node_id, initial_canvas_x, initial_canvas_y, initial_node_x, initial_node_y)`
    dragging_node: Option<(String, f32, f32, f32, f32)>,
    /// Connecting a wire state
    connecting_wire: Option<WireDragState>,
    /// Selected node ID
    selected_node: Option<String>,
    /// Canvas pan offset (X, Y)
    pan_offset: (f32, f32),
    /// Canvas panning: `(initial_canvas_x, initial_canvas_y, initial_pan_x, initial_pan_y)`
    panning_canvas: Option<(f32, f32, f32, f32)>,
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
            _subscription: sub,
            dragging_node: None,
            connecting_wire: None,
            selected_node: None,
            pan_offset: (0.0, 0.0),
            panning_canvas: None,
        }
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

    /// Add a new node of the given kind at a convenient position.
    pub fn add_node(&mut self, kind: NodeKind, cx: &mut Context<Self>) {
        let count = self.graph.nodes.len() + 1;
        let id = format!("node_{}_{}", kind.title().to_lowercase().replace(' ', "_"), count);
        // Position staggered in canvas view
        let offset_x = 220.0 + ((self.graph.nodes.len() as f32 % 5.0) * 40.0) - self.pan_offset.0;
        let offset_y = 120.0 + ((self.graph.nodes.len() as f32 % 5.0) * 35.0) - self.pan_offset.1;

        let node = ModifierNode::new(id, offset_x, offset_y, kind);
        self.graph.add_node(node);
        self.sync_to_state(cx);
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
    fn try_finish_wire_connection(
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

        // Find closest compatible socket within snap radius (32.0 px)
        let snap_radius_sq = 32.0 * 32.0;
        let mut best_candidate = None;
        let mut best_dist_sq = snap_radius_sq;

        for node in &self.graph.nodes {
            if node.id == wire.node_id {
                continue; // Cannot connect a node to itself
            }
            let (nx, ny) = (node.pos_x + pan_x, node.pos_y + pan_y);

            if !wire.is_from_input {
                // Dragging from OUTPUT socket -> seeking an INPUT socket
                for (idx, in_name) in node.kind.input_sockets().iter().enumerate() {
                    let sx = nx + 14.0;
                    let sy = ny + 41.0 + (idx as f32 * 26.0);
                    let dx = target_canvas_x - sx;
                    let dy = target_canvas_y - sy;
                    let dsq = dx * dx + dy * dy;
                    if dsq < best_dist_sq {
                        best_dist_sq = dsq;
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
                    let dsq = dx * dx + dy * dy;
                    if dsq < best_dist_sq {
                        best_dist_sq = dsq;
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (factor, base_val, evaluated_out) = {
            let s = self.editor_state.read(cx);
            let factor = s.get_layer_progression_factor(&self.layer_id);
            let base = s.get_layer_property_base_value(&self.layer_id, &self.prop_path);
            let out = self.graph.evaluate(base, factor);
            (factor, base, out)
        };

        let entity = cx.entity().clone();

        // 1. Top Header & Toolbar
        let header = self.render_header(factor, base_val, evaluated_out, &entity, cx);

        // 2. Node Canvas Area
        let canvas = self.render_canvas(&entity, cx);

        v_flex()
            .id("modifier_graph_root")
            .test_support()
            .size_full()
            .bg(cx.theme().background)
            .track_focus(&self.focus_handle)
            .child(header)
            .child(canvas)
    }
}

impl ModifierGraphView {
    fn render_header(
        &self,
        factor: f32,
        base_val: f32,
        evaluated_out: f32,
        entity: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Div {
        let ent_math = entity.clone();
        let ent_lerp = entity.clone();
        let ent_clamp = entity.clone();
        let ent_remap = entity.clone();
        let ent_smooth = entity.clone();
        let ent_noise = entity.clone();
        let ent_wave = entity.clone();
        let ent_step = entity.clone();
        let ent_const = entity.clone();
        let ent_reset = entity.clone();

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
                                    .child(format!(
                                        "Modifier Graph: {}  (Layer: {})",
                                        self.prop_path, self.layer_id
                                    )),
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
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(0x28231a))
                                    .border_1()
                                    .border_color(rgb(0xd97706))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(rgb(0xfcd34d))
                                            .child("Base Value:"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_bold()
                                            .text_color(rgb(0xfbbf24))
                                            .child(format!("{base_val:.2}")),
                                    ),
                            )
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
                // Row 2: Node Addition Toolbar
                h_flex()
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .gap_1p5()
                    .items_center()
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("+ Add Node:"),
                    )
                    .child(self.toolbar_btn("Math", ent_math, move |this, cx| {
                        this.add_node(
                            NodeKind::Math {
                                op: MathOp::Multiply,
                                default_b: 1.0,
                            },
                            cx,
                        );
                    }, cx))
                    .child(self.toolbar_btn("Lerp", ent_lerp, move |this, cx| {
                        this.add_node(
                            NodeKind::Lerp {
                                default_a: 0.0,
                                default_b: 1.0,
                            },
                            cx,
                        );
                    }, cx))
                    .child(self.toolbar_btn("Clamp", ent_clamp, move |this, cx| {
                        this.add_node(NodeKind::Clamp { min: 0.0, max: 1.0 }, cx);
                    }, cx))
                    .child(self.toolbar_btn("Remap", ent_remap, move |this, cx| {
                        this.add_node(
                            NodeKind::Remap {
                                in_min: 0.0,
                                in_max: 1.0,
                                out_min: 0.0,
                                out_max: 100.0,
                            },
                            cx,
                        );
                    }, cx))
                    .child(self.toolbar_btn("Smoothstep", ent_smooth, move |this, cx| {
                        this.add_node(NodeKind::Smoothstep { edge0: 0.0, edge1: 1.0 }, cx);
                    }, cx))
                    .child(self.toolbar_btn("Noise", ent_noise, move |this, cx| {
                        this.add_node(
                            NodeKind::Noise {
                                frequency: 2.0,
                                amplitude: 50.0,
                            },
                            cx,
                        );
                    }, cx))
                    .child(self.toolbar_btn("Wave", ent_wave, move |this, cx| {
                        this.add_node(
                            NodeKind::Wave {
                                wave_type: WaveType::Sine,
                                frequency: 1.0,
                                amplitude: 50.0,
                                offset: 0.0,
                            },
                            cx,
                        );
                    }, cx))
                    .child(self.toolbar_btn("Stepped", ent_step, move |this, cx| {
                        this.add_node(NodeKind::Stepped { steps: 5.0 }, cx);
                    }, cx))
                    .child(self.toolbar_btn("Constant", ent_const, move |this, cx| {
                        this.add_node(NodeKind::Constant { value: 1.0 }, cx);
                    }, cx))
                    .child(div().flex_grow(1.0))
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
                                ent_reset.update(cx, |this, cx| {
                                    this.reset_graph(cx);
                                });
                            })
                            .child("Reset Graph"),
                    ),
            )
    }

    fn toolbar_btn<F>(&self, label: &'static str, entity: Entity<Self>, on_click: F, cx: &App) -> Div
    where
        F: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        div()
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
                entity.update(cx, |this, cx| {
                    on_click(this, cx);
                });
            })
            .child(label)
    }

    fn render_canvas(&mut self, entity: &Entity<Self>, cx: &mut Context<Self>) -> Stateful<Div> {
        let (pan_x, pan_y) = self.pan_offset;

        let ent_drag = entity.clone();
        let ent_up = entity.clone();
        let ent_pan_down = entity.clone();
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
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgb(0x101116))
            // Click empty canvas cancels pending wire or deselects node
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                ent_canvas_down.update(cx, |this, cx| {
                    if this.connecting_wire.is_some() {
                        let (pan_x, pan_y) = this.pan_offset;
                        let connected = this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                        if !connected {
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
                    // Wire drag
                    if let Some(wire) = &mut this.connecting_wire {
                        wire.cur_x = curr_x;
                        wire.cur_y = curr_y;
                        changed = true;
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
                let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                ent_up.update(cx, |this, cx| {
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
            });

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

    /// Build a smooth bezier wire between two points.
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

        let mut elements = Vec::with_capacity(25);
        let steps = 22;

        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let it = 1.0 - t;
            let bx = it * it * it * p0.0 + 3.0 * it * it * t * p1.0 + 3.0 * it * t * t * p2.0 + t * t * t * p3.0;
            let by = it * it * it * p0.1 + 3.0 * it * it * t * p1.1 + 3.0 * it * t * t * p2.1 + t * t * t * p3.1;

            let bead = div()
                .absolute()
                .left(px(bx - 3.0))
                .top(px(by - 3.0))
                .w(px(6.0))
                .h(px(6.0))
                .rounded_full()
                .bg(bead_color);

            elements.push(bead.into_any_element());
        }

        // Render disconnect badge at the midpoint of the wire
        if let Some((to_node, to_sock)) = disconnect_target {
            let mx = (x1 + x2) * 0.5;
            let my = (y1 + y2) * 0.5;

            let ent_disc = entity.clone();
            let tn = to_node.clone();
            let ts = to_sock.clone();

            let disconnect_btn = div()
                .id(SharedString::from(format!("disc_{}_{}", to_node, to_sock)))
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
            NodeKind::Math { .. } | NodeKind::Lerp { .. } | NodeKind::Clamp { .. } | NodeKind::Remap { .. } | NodeKind::Smoothstep { .. } => {
                (rgb(0x581c87), rgb(0xd8b4fe)) // Deep purple
            }
            NodeKind::Noise { .. } | NodeKind::Wave { .. } | NodeKind::Stepped { .. } => {
                (rgb(0x14532d), rgb(0x86efac)) // Deep green
            }
            NodeKind::Output => {
                (rgb(0x78350f), rgb(0xfde68a)) // Deep amber
            }
        };

        let ent_drag = entity.clone();
        let ent_del = entity.clone();
        let nid_drag = node_id.clone();
        let nid_del = node_id.clone();
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
            .shadow_md();

        // 1. Header with title, drag handler, and delete button
        let is_output = matches!(node.kind, NodeKind::Output);
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
                    .child(node.title.clone()),
            )
            .child(if !is_output {
                div()
                    .id(SharedString::from(format!("del_node_{}", node.id)))
                    .cursor_pointer()
                    .text_xs()
                    .text_color(rgb(0x9ca3af))
                    .hover(|s| s.text_color(rgb(0xef4444)))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
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

        h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .id(SharedString::from(format!("in_{}_{}", node_id, socket_name)))
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
                    .hover(|s| s.bg(rgb(0x0ea5e9)).border_color(rgb(0xffffff)))
                    .cursor_pointer()
                    .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
                        let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                        ent_up.update(cx, |this, cx| {
                            let (pan_x, pan_y) = this.pan_offset;
                            this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                        });
                    })
                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                        let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                        ent_down.update(cx, |this, cx| {
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
                        ent_rclick.update(cx, |this, cx| {
                            this.disconnect_socket(&nid_rclick, &sname_rclick, cx);
                        });
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x9ca3af))
                    .child(socket_name),
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

        h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x9ca3af))
                    .child(socket_name),
            )
            .child(
                div()
                    .id(SharedString::from(format!("out_{}_{}", node_id, socket_name)))
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
                    .hover(|s| s.bg(rgb(0x059669)).border_color(rgb(0xffffff)))
                    .cursor_pointer()
                    .on_mouse_up(MouseButton::Left, move |event, _window, cx| {
                        let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                        ent_up.update(cx, |this, cx| {
                            let (pan_x, pan_y) = this.pan_offset;
                            this.try_finish_wire_connection(curr_x, curr_y, pan_x, pan_y, cx);
                        });
                    })
                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                        let (curr_x, curr_y) = event_to_canvas_pos(event.position);
                        ent_down.update(cx, |this, cx| {
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
                        ent_rclick.update(cx, |this, cx| {
                            this.graph.connections.retain(|c| !(c.from_node == nid_rclick && c.from_socket == sname_rclick));
                            this.sync_to_state(cx);
                        });
                    }),
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
            NodeKind::Constant { value } => {
                let v = *value;
                let ent_dec = ent.clone();
                let ent_inc = ent.clone();
                let nid_dec = nid.clone();
                let nid_inc = nid.clone();

                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Value:"))
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(self.param_step_btn("-", ent_dec, move |this, cx| {
                                if let Some(n) = this.graph.get_node_mut(&nid_dec) {
                                    if let NodeKind::Constant { value } = &mut n.kind {
                                        *value -= 1.0;
                                        this.sync_to_state(cx);
                                    }
                                }
                            }, cx))
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(rgb(0x232430))
                                    .text_xs()
                                    .font_medium()
                                    .text_color(rgb(0x38bdf8))
                                    .child(format!("{v:.2}")),
                            )
                            .child(self.param_step_btn("+", ent_inc, move |this, cx| {
                                if let Some(n) = this.graph.get_node_mut(&nid_inc) {
                                    if let NodeKind::Constant { value } = &mut n.kind {
                                        *value += 1.0;
                                        this.sync_to_state(cx);
                                    }
                                }
                            }, cx)),
                    )
            }

            NodeKind::Math { op, default_b } => {
                let current_op = *op;
                let def_b = *default_b;
                let ent_b_dec = ent.clone();
                let ent_b_inc = ent.clone();
                let nid_b_dec = nid.clone();
                let nid_b_inc = nid.clone();

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
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(self.param_step_btn("-", ent_b_dec, move |this, cx| {
                                        if let Some(n) = this.graph.get_node_mut(&nid_b_dec) {
                                            if let NodeKind::Math { default_b, .. } = &mut n.kind {
                                                *default_b -= 0.5;
                                                this.sync_to_state(cx);
                                            }
                                        }
                                    }, cx))
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(rgb(0x232430))
                                            .text_xs()
                                            .font_medium()
                                            .text_color(rgb(0xc084fc))
                                            .child(format!("{def_b:.2}")),
                                    )
                                    .child(self.param_step_btn("+", ent_b_inc, move |this, cx| {
                                        if let Some(n) = this.graph.get_node_mut(&nid_b_inc) {
                                            if let NodeKind::Math { default_b, .. } = &mut n.kind {
                                                *default_b += 0.5;
                                                this.sync_to_state(cx);
                                            }
                                        }
                                    }, cx)),
                            ),
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
                let ent_f_dec = ent.clone();
                let ent_f_inc = ent.clone();
                let nid_f_dec = nid.clone();
                let nid_f_inc = nid.clone();

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
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(self.param_step_btn("-", ent_f_dec, move |this, cx| {
                                        if let Some(n) = this.graph.get_node_mut(&nid_f_dec) {
                                            if let NodeKind::Wave { frequency, .. } = &mut n.kind {
                                                *frequency = (*frequency - 0.5).max(0.1);
                                                this.sync_to_state(cx);
                                            }
                                        }
                                    }, cx))
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(rgb(0x232430))
                                            .text_xs()
                                            .font_medium()
                                            .text_color(rgb(0x4ade80))
                                            .child(format!("{freq:.1}x")),
                                    )
                                    .child(self.param_step_btn("+", ent_f_inc, move |this, cx| {
                                        if let Some(n) = this.graph.get_node_mut(&nid_f_inc) {
                                            if let NodeKind::Wave { frequency, .. } = &mut n.kind {
                                                *frequency += 0.5;
                                                this.sync_to_state(cx);
                                            }
                                        }
                                    }, cx)),
                            ),
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
                let ent_dec = ent.clone();
                let ent_inc = ent.clone();
                let nid_dec = nid.clone();
                let nid_inc = nid.clone();

                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .border_t_1()
                    .border_color(rgb(0x2e303e))
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Steps:"))
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(self.param_step_btn("-", ent_dec, move |this, cx| {
                                if let Some(n) = this.graph.get_node_mut(&nid_dec) {
                                    if let NodeKind::Stepped { steps } = &mut n.kind {
                                        *steps = (*steps - 1.0).max(1.0);
                                        this.sync_to_state(cx);
                                    }
                                }
                            }, cx))
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(rgb(0x232430))
                                    .text_xs()
                                    .font_medium()
                                    .text_color(rgb(0x4ade80))
                                    .child(format!("{st:.0}")),
                            )
                            .child(self.param_step_btn("+", ent_inc, move |this, cx| {
                                if let Some(n) = this.graph.get_node_mut(&nid_inc) {
                                    if let NodeKind::Stepped { steps } = &mut n.kind {
                                        *steps += 1.0;
                                        this.sync_to_state(cx);
                                    }
                                }
                            }, cx)),
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

    fn param_step_btn<F>(&self, label: &'static str, entity: Entity<Self>, on_click: F, cx: &App) -> Div
    where
        F: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        div()
            .w(px(16.0))
            .h(px(16.0))
            .rounded_sm()
            .bg(rgb(0x232430))
            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_xs()
            .font_bold()
            .text_color(cx.theme().foreground)
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                entity.update(cx, |this, cx| {
                    on_click(this, cx);
                });
            })
            .child(label)
    }
}
