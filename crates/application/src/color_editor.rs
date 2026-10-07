//! Reference-styled color editor: SV field, hue/alpha/RGB sliders, numeric
//! fields, hex entry, NEW/ORIG swatches. No preset swatches anywhere — every
//! control reads and commits the live model color.
//!
//! One [`InspectorColorPicker`] entity per color usage owns its tab, input
//! states, drag bounds, and ORIG snapshot; the caller supplies `current` and
//! a `commit` closure, so the same component serves solid fills, gradient
//! stops, and every effect color row.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::base::{ElementExt as _, TestSupportExt as _, h_flex, v_flex};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::{
    div, px, linear_color_stop, linear_gradient, AnyElement, App, AppContext as _, Bounds,
    Context, Entity, Focusable as _, Hsla, InteractiveElement, IntoElement as _, LinearColorStop,
    MouseButton, ParentElement, Pixels, Point, Rgba, SharedString,
    Styled, Subscription, Window,
};
use project::Color;

/// Value-format tabs. ACEScg is deliberately absent: the pipeline has no
/// ACES backend, so that tab would be decorative.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ColorEditorTab {
    #[default]
    Rgb,
    Hex,
    Hsv,
}

/// Draggable control ids for bounds + active-drag tracking.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DragCtl {
    Sv,
    Hue,
    Alpha,
    R,
    G,
    B,
}

/// Numeric input fields owned by the editor.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Field {
    Hex,
    R,
    G,
    B,
    H,
    S,
    V,
}

/// Model commit: writes the picked color (live) and notifies.
type CommitFn = Rc<dyn Fn(Color, &mut App)>;

pub(crate) struct InspectorColorPicker {
    commit: CommitFn,
    tab: ColorEditorTab,
    /// Compact rows collapse the editor behind a toggle; inline sections
    /// (solid fill) render it unconditionally and ignore this.
    pub expanded: bool,
    orig: Option<Color>,
    /// Last model color seen at render, shared with field subscriptions
    /// through interior mutability (render holds an outer `state` guard, so
    /// no `cx` borrowing is possible here).
    current: Rc<RefCell<Color>>,
    dragging: Option<DragCtl>,
    sv_bounds: Option<Bounds<Pixels>>,
    bar_bounds: HashMap<DragCtl, Bounds<Pixels>>,
    inputs: HashMap<Field, Entity<InputState>>,
    _subs: Vec<Subscription>,
}

impl InspectorColorPicker {
    /// Build the per-usage editor state. `commit` writes the picked color to
    /// the model (live, like the previous picker); numeric fields parse and
    /// commit on change. Call from a `use_keyed_state` init closure, which
    /// supplies the entity context.
    pub fn new<F>(window: &mut Window, cx: &mut Context<Self>, commit: F) -> Self
    where
        F: Fn(Color, &mut App) + 'static,
    {
        let commit: CommitFn = Rc::new(commit);
        let mut inputs = HashMap::new();
        for field in [Field::Hex, Field::R, Field::G, Field::B, Field::H, Field::S, Field::V] {
            inputs.insert(field, cx.new(|cx| InputState::new(window, cx)));
        }
        let mut this = Self {
            commit,
            tab: ColorEditorTab::Rgb,
            expanded: false,
            orig: None,
            current: Rc::new(RefCell::new(Color::WHITE)),
            dragging: None,
            sv_bounds: None,
            bar_bounds: HashMap::new(),
            inputs,
            _subs: Vec::new(),
        };
        // Input subscriptions commit parsed values; unparseable text is
        // ignored so mid-typing states ("2", "#FF0") never corrupt the model.
        for (field, input) in this.inputs.clone() {
            let sub = cx.subscribe(
                &input,
                move |editor: &mut Self, input: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let text = input.read(cx).value().to_string();
                    editor.apply_field(field, &text, cx);
                },
            );
            this._subs.push(sub);
        }
        this
    }

    /// Snapshot the ORIG swatch (call when the editor opens).
    pub fn snapshot_orig(&mut self, color: Color) {
        self.orig = Some(color);
    }

    pub fn set_expanded(&mut self, open: bool, current: Color) {
        self.expanded = open;
        if open {
            self.snapshot_orig(current);
        } else {
            self.orig = None;
            self.dragging = None;
        }
    }

    fn commit(&self, color: Color, cx: &mut App) {
        (self.commit)(color, cx);
    }

    /// Parse one numeric field against the last rendered color and commit
    /// when it yields a different color. Returns true when committed.
    fn apply_field(&self, field: Field, text: &str, cx: &mut Context<Self>) -> bool {
        let c = *self.current.borrow();
        let (r, g, b) = (c.r, c.g, c.b);
        let next = match field {
            Field::Hex => match Color::from_hex(text.trim()) {
                Ok(nc) => Color::rgba(nc.r, nc.g, nc.b, c.a),
                Err(_) => return false,
            },
            Field::R => match parse_u8(text) {
                Some(v) => Color::rgba(v as f32 / 255.0, g, b, c.a),
                None => return false,
            },
            Field::G => match parse_u8(text) {
                Some(v) => Color::rgba(r, v as f32 / 255.0, b, c.a),
                None => return false,
            },
            Field::B => match parse_u8(text) {
                Some(v) => Color::rgba(r, g, v as f32 / 255.0, c.a),
                None => return false,
            },
            Field::H => {
                let (_, s, v) = rgb_to_hsv(r, g, b);
                match text.trim().parse::<f32>() {
                    Ok(h) if (0.0..=360.0).contains(&h) => {
                        let (nr, ng, nb) = hsv_to_rgb(h.rem_euclid(360.0), s, v);
                        Color::rgba(nr, ng, nb, c.a)
                    }
                    _ => return false,
                }
            }
            Field::S => {
                let (h, _, v) = rgb_to_hsv(r, g, b);
                match text.trim().parse::<f32>() {
                    Ok(s) if (0.0..=100.0).contains(&s) => {
                        let (nr, ng, nb) = hsv_to_rgb(h, s / 100.0, v);
                        Color::rgba(nr, ng, nb, c.a)
                    }
                    _ => return false,
                }
            }
            Field::V => {
                let (h, s, _) = rgb_to_hsv(r, g, b);
                match text.trim().parse::<f32>() {
                    Ok(v) if (0.0..=100.0).contains(&v) => {
                        let (nr, ng, nb) = hsv_to_rgb(h, s, v / 100.0);
                        Color::rgba(nr, ng, nb, c.a)
                    }
                    _ => return false,
                }
            }
        };
        if colors_equal(next, c) {
            return false;
        }
        self.commit(next, cx);
        true
    }
}

/// Strict 0..=255 integer parse (no floats, no signs, no whitespace
/// surprises): mid-typing text simply doesn't commit.
fn parse_u8(text: &str) -> Option<u8> {
    let t = text.trim();
    if t.is_empty() || t.len() > 3 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: u16 = t.parse().ok()?;
    if v > 255 {
        return None;
    }
    Some(v as u8)
}

fn colors_equal(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 1e-6
        && (a.g - b.g).abs() < 1e-6
        && (a.b - b.b).abs() < 1e-6
        && (a.a - b.a).abs() < 1e-6
}

/// RGB (0..1) to HSV (h degrees, s/v 0..1).
pub(crate) fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d.abs() < 1e-6 {
        0.0
    } else if (mx - r).abs() < 1e-6 {
        60.0 * (((g - b) / d) % 6.0)
    } else if (mx - g).abs() < 1e-6 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if mx.abs() < 1e-6 { 0.0 } else { d / mx };
    (h.rem_euclid(360.0), s.clamp(0.0, 1.0), mx.clamp(0.0, 1.0))
}

/// HSV (h degrees, s/v 0..1) to RGB (0..1).
pub(crate) fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let hh = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if hh < 1.0 {
        (c, x, 0.0)
    } else if hh < 2.0 {
        (x, c, 0.0)
    } else if hh < 3.0 {
        (0.0, c, x)
    } else if hh < 4.0 {
        (0.0, x, c)
    } else if hh < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (r + m, g + m, b + m)
}

fn u8_tuple(c: Color) -> (u8, u8, u8) {
    (
        (c.r * 255.0).round().clamp(0.0, 255.0) as u8,
        (c.g * 255.0).round().clamp(0.0, 255.0) as u8,
        (c.b * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn hex_str(c: Color) -> String {
    let (r, g, b) = u8_tuple(c);
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Gradient stop from straight RGBA (visual only; exactness irrelevant).
fn stop(r: f32, g: f32, b: f32, a: f32, at: f32) -> LinearColorStop {
    let hsla: Hsla = Rgba { r, g, b, a }.into();
    linear_color_stop(hsla, at)
}

/// Render the reference-styled editor inline. `id` prefixes test ids.
/// `current` is the live model color; everything commits through the
/// entity's closure. Takes `&App` only: all mutation happens in event
/// handlers (which own their `&mut App`) or through the `RefCell` mirror.
pub(crate) fn render_color_editor(
    editor: &Entity<InspectorColorPicker>,
    id: &str,
    current: Color,
    cx: &App,
) -> AnyElement {
    // Silent model mirror for field edits (interior mutability: no `cx`
    // borrow, so the outer render guards never conflict).
    editor.read(cx).current.replace(current);

    let (tab, orig, sv_bounds) = {
        let this = editor.read(cx);
        (this.tab, this.orig, this.sv_bounds)
    };
    let (r8, g8, b8) = u8_tuple(current);
    let (h, s, v) = rgb_to_hsv(current.r, current.g, current.b);
    let (hr, hg, hb) = hsv_to_rgb(h, 1.0, 1.0);
    let hue_full = Color::rgba(hr, hg, hb, 1.0);

    // --- format tabs (RGB | HEX | HSV): all switch real sections ---
    let mut tabs = h_flex().gap_1().items_center().text_xs();
    for (t, label, tid) in [
        (ColorEditorTab::Rgb, "RGB", "tab_rgb"),
        (ColorEditorTab::Hex, "HEX", "tab_hex"),
        (ColorEditorTab::Hsv, "HSV", "tab_hsv"),
    ] {
        let ed = editor.clone();
        let active = tab == t;
        tabs = tabs.child(
            div()
                .id(SharedString::from(format!("{id}_{tid}")))
                .test_support()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if active { cx.theme().accent_foreground } else { cx.theme().muted_foreground })
                .bg(if active { cx.theme().accent } else { cx.theme().transparent })
                .hover(|s| s.text_color(cx.theme().foreground))
                .child(label)
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    ed.update(cx, |this, cx| {
                        this.tab = t;
                        cx.notify();
                    });
                }),
        );
    }

    // --- NEW / ORIG swatches + readouts (ORIG click reverts) ---
    let ed_orig = editor.clone();
    let orig_color = orig.unwrap_or(current);
    let new_orig = h_flex()
        .gap_2()
        .items_center()
        .child(
            v_flex()
                .w(px(40.))
                .overflow_hidden()
                .rounded_sm()
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .h(px(20.))
                        .bg(Rgba { r: current.r, g: current.g, b: current.b, a: 1.0 })
                        .child(
                            div()
                                .px_1()
                                .text_xs()
                                .text_color(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.9 })
                                .child("NEW"),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{id}_orig_swatch")))
                        .test_support()
                        .h(px(20.))
                        .bg(Rgba { r: orig_color.r, g: orig_color.g, b: orig_color.b, a: 1.0 })
                        .cursor_pointer()
                        .child(
                            div()
                                .px_1()
                                .text_xs()
                                .text_color(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.9 })
                                .child("ORIG"),
                        )
                        .on_mouse_down(MouseButton::Left, move |_e, window, cx| {
                            let back = ed_orig.read(cx).orig;
                            if let Some(oc) = back {
                                InspectorColorPicker::commit_synced(&ed_orig, oc, None, window, cx);
                            }
                        }),
                ),
        )
        .child(
            v_flex()
                .gap_1()
                .flex_1()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .text_xs()
                        .child(div().text_color(cx.theme().muted_foreground).child("RGB"))
                        .child(div().text_color(cx.theme().foreground).child(format!("{r8}, {g8}, {b8}"))),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .text_xs()
                        .child(div().text_color(cx.theme().muted_foreground).child("HEX"))
                        .child(div().text_color(cx.theme().foreground).child(hex_str(current)))
                        .child(div().text_color(cx.theme().muted_foreground).child("8-bit")),
                ),
        );

    // --- SV field: hue base + white overlay + black overlay + crosshair ---
    let ed_sv = editor.clone();
    let ed_sv_move = editor.clone();
    let ed_sv_up = editor.clone();
    let ed_sv_out = editor.clone();
    let mut sv = div()
        .relative()
        .w_full()
        .h(px(160.))
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(cx.theme().border)
        .cursor_pointer()
        .bg(Rgba { r: hue_full.r, g: hue_full.g, b: hue_full.b, a: 1.0 })
        .child(
            div()
                .absolute()
                .inset_0()
                .bg(linear_gradient(90.0, stop(1.0, 1.0, 1.0, 1.0, 0.0), stop(1.0, 1.0, 1.0, 0.0, 1.0))),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .bg(linear_gradient(180.0, stop(0.0, 0.0, 0.0, 0.0, 0.0), stop(0.0, 0.0, 0.0, 1.0, 1.0))),
        )
        .on_prepaint({
            let ed = editor.clone();
            move |bounds, _, cx| {
                ed.update(cx, |this, _| {
                    this.sv_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            InspectorColorPicker::begin_drag(&ed_sv, DragCtl::Sv, event.position, window, cx);
        })
        .on_mouse_move(move |event, window, cx| {
            if ed_sv_move.read(cx).dragging == Some(DragCtl::Sv) {
                InspectorColorPicker::drag_to(&ed_sv_move, DragCtl::Sv, event.position, window, cx);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_e, _w, cx| {
            InspectorColorPicker::end_drag(&ed_sv_up, cx);
        })
        .on_mouse_up_out(MouseButton::Left, move |_e, _w, cx| {
            InspectorColorPicker::end_drag(&ed_sv_out, cx);
        })
        .id(SharedString::from(format!("{id}_sv_field")))
        .test_support();
    // Crosshair ring from last measured bounds (exact px, like gizmo dots).
    if let Some(b) = sv_bounds {
        let px_x = s * f32::from(b.size.width);
        let px_y = (1.0 - v) * f32::from(b.size.height);
        sv = sv.child(
            div()
                .absolute()
                .left(px(px_x - 6.0))
                .top(px(px_y - 6.0))
                .w(px(12.))
                .h(px(12.))
                .rounded_full()
                .border_2()
                .border_color(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }),
        );
    }

    // --- sliders: H + A always visible; RGB bars live in the RGB tab ---
    let hue_bar = slider_row(editor, id, DragCtl::Hue, "H", hue_track(), h / 360.0, &format!("{h:.0}°"), cx);
    let alpha_bar = slider_row(
        editor,
        id,
        DragCtl::Alpha,
        "A",
        div()
            .absolute()
            .inset_0()
            .bg(Rgba { r: 0.5, g: 0.5, b: 0.5, a: 1.0 })
            .child(gradient_track(
                Rgba { r: current.r, g: current.g, b: current.b, a: 0.0 },
                Rgba { r: current.r, g: current.g, b: current.b, a: 1.0 },
                90.0,
            ))
            .into_any_element(),
        current.a,
        &format!("{:.0}%", current.a * 100.0),
        cx,
    );

    // --- numeric section per format tab ---
    let numeric: AnyElement = match tab {
        ColorEditorTab::Rgb => {
            let mut rows = v_flex().gap_1p5();
            for (ctl, label, val, field, fid) in [
                (DragCtl::R, "R", r8, Field::R, "num_r"),
                (DragCtl::G, "G", g8, Field::G, "num_g"),
                (DragCtl::B, "B", b8, Field::B, "num_b"),
            ] {
                let from = Rgba { r: if ctl == DragCtl::R { 0.0 } else { current.r }, g: if ctl == DragCtl::G { 0.0 } else { current.g }, b: if ctl == DragCtl::B { 0.0 } else { current.b }, a: 1.0 };
                let to = Rgba { r: if ctl == DragCtl::R { 1.0 } else { current.r }, g: if ctl == DragCtl::G { 1.0 } else { current.g }, b: if ctl == DragCtl::B { 1.0 } else { current.b }, a: 1.0 };
                rows = rows.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().w(px(10.)).text_xs().text_color(cx.theme().muted_foreground).child(label))
                        .child(div().flex_1().child(slider_bar(editor, id, ctl, gradient_track(from, to, 90.0), val as f32 / 255.0, cx)))
                        .child(
                            div()
                                .w(px(52.))
                                .id(SharedString::from(format!("{id}_{fid}")))
                                .test_support()
                                .child(Input::new(&editor.read(cx).input_entity(field)).small()),
                        ),
                );
            }
            rows.into_any_element()
        }
        ColorEditorTab::Hex => v_flex()
            .gap_1p5()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("HEX"))
                    .child(
                        div()
                            .flex_1()
                            .id(SharedString::from(format!("{id}_hex_input")))
                            .test_support()
                            .child(Input::new(&editor.read(cx).input_entity(Field::Hex))),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("8-bit per channel, e.g. #FFFB00"),
            )
            .into_any_element(),
        ColorEditorTab::Hsv => {
            let mut rows = v_flex().gap_1p5();
            for (label, field, fid, max) in [
                ("H", Field::H, "num_h", "0–360°"),
                ("S", Field::S, "num_s", "0–100%"),
                ("V", Field::V, "num_v", "0–100%"),
            ] {
                rows = rows.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().w(px(10.)).text_xs().text_color(cx.theme().muted_foreground).child(label))
                        .child(
                            div()
                                .flex_1()
                                .id(SharedString::from(format!("{id}_{fid}")))
                                .test_support()
                                .child(Input::new(&editor.read(cx).input_entity(field))),
                        )
                        .child(div().w(px(52.)).text_xs().text_right().text_color(cx.theme().muted_foreground).child(max)),
                );
            }
            rows.into_any_element()
        }
    };

    v_flex()
        .gap_2()
        .child(tabs)
        .child(new_orig)
        .child(sv)
        .child(hue_bar)
        .child(alpha_bar)
        .child(numeric)
        .into_any_element()
}

/// 24-segment rainbow track (linear gradients are two-stop only, so the
/// hue bar composites discrete segments).
fn hue_track() -> AnyElement {
    let mut row = h_flex().absolute().inset_0().overflow_hidden().rounded_sm();
    for i in 0..24 {
        let (r, g, b) = hsv_to_rgb(i as f32 * 15.0, 1.0, 1.0);
        row = row.child(div().flex_1().h_full().bg(Rgba { r, g, b, a: 1.0 }));
    }
    row.into_any_element()
}

/// One slider row: label + draggable gradient bar with knob + readout.
#[allow(clippy::too_many_arguments)]
fn slider_row(
    editor: &Entity<InspectorColorPicker>,
    id: &str,
    ctl: DragCtl,
    label: &str,
    track: AnyElement,
    t: f32,
    readout: &str,
    cx: &App,
) -> AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .child(div().w(px(10.)).text_xs().text_color(cx.theme().muted_foreground).child(label.to_string()))
        .child(div().flex_1().child(slider_bar(editor, id, ctl, track, t, cx)))
        .child(
            div()
                .w(px(44.))
                .text_xs()
                .text_right()
                .text_color(cx.theme().foreground)
                .child(readout.to_string()),
        )
        .into_any_element()
}

/// Draggable bar: track element + knob from last measured bounds + full
/// drag handling. Knob skips the first frame (no bounds yet).
fn slider_bar(
    editor: &Entity<InspectorColorPicker>,
    id: &str,
    ctl: DragCtl,
    track: AnyElement,
    t: f32,
    cx: &App,
) -> AnyElement {
    let ed_down = editor.clone();
    let ed_move = editor.clone();
    let ed_up = editor.clone();
    let ed_out = editor.clone();
    let ed_pre = editor.clone();
    let ctl_name = match ctl {
        DragCtl::Hue => "hue",
        DragCtl::Alpha => "alpha",
        DragCtl::R => "r",
        DragCtl::G => "g",
        DragCtl::B => "b",
        DragCtl::Sv => "sv",
    };
    let mut bar = div()
        .relative()
        .w_full()
        .h(px(20.))
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .top(px(8.))
                .h(px(4.))
                .rounded_sm()
                .overflow_hidden()
                .child(track),
        )
        .on_prepaint(move |bounds, _, cx| {
            ed_pre.update(cx, |this, _| {
                this.bar_bounds.insert(ctl, bounds);
            });
        })
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            InspectorColorPicker::begin_drag(&ed_down, ctl, event.position, window, cx);
        })
        .on_mouse_move(move |event, window, cx| {
            if ed_move.read(cx).dragging == Some(ctl) {
                InspectorColorPicker::drag_to(&ed_move, ctl, event.position, window, cx);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_e, _w, cx| {
            InspectorColorPicker::end_drag(&ed_up, cx);
        })
        .on_mouse_up_out(MouseButton::Left, move |_e, _w, cx| {
            InspectorColorPicker::end_drag(&ed_out, cx);
        })
        .id(SharedString::from(format!("{id}_{ctl_name}_bar")))
        .test_support();
    if let Some(b) = editor.read(cx).bar_bounds.get(&ctl).copied() {
        let w = f32::from(b.size.width);
        bar = bar.child(
            div()
                .absolute()
                .left(px((t * w - 5.0).clamp(0.0, (w - 10.0).max(0.0))))
                .top(px(1.))
                .w(px(10.))
                .h(px(18.))
                .rounded_sm()
                .bg(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
                .border_1()
                .border_color(Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.5 }),
        );
    }
    bar.into_any_element()
}

/// Two-stop gradient track filling its parent.
fn gradient_track(from: Rgba, to: Rgba, angle: f32) -> AnyElement {
    let f: Hsla = from.into();
    let t: Hsla = to.into();
    div().absolute().inset_0().bg(linear_gradient(angle, linear_color_stop(f, 0.0), linear_color_stop(t, 1.0))).into_any_element()
}

impl InspectorColorPicker {
    fn input_entity(&self, field: Field) -> Entity<InputState> {
        self.inputs.get(&field).cloned().expect("color field input registered")
    }

    fn bounds_for(&self, ctl: DragCtl) -> Option<Bounds<Pixels>> {
        if ctl == DragCtl::Sv {
            self.sv_bounds
        } else {
            self.bar_bounds.get(&ctl).copied()
        }
    }

    /// Commit + resync every numeric field except the one being typed
    /// (`except` is `None` for drag/slider commits, which own no field).
    fn commit_synced(
        editor: &Entity<Self>,
        color: Color,
        except: Option<Field>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let is_same = {
            let this = editor.read(cx);
            colors_equal(color, *this.current.borrow())
        };
        if is_same {
            return;
        }
        editor.update(cx, |this, cx| {
            (this.commit)(color, cx);
            *this.current.borrow_mut() = color;
            cx.notify();
        });
        Self::sync_inputs(editor, except, window, cx);
    }

    /// Push the committed color into every numeric field (except the focused
    /// one being typed, and except `skip`). Render also calls this for
    /// never-touched (empty) inputs so boxes are never blank.
    pub(crate)     fn sync_inputs(
        editor: &Entity<Self>,
        skip: Option<Field>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (current, inputs) = {
            let this = editor.read(cx);
            (*this.current.borrow(), this.inputs.clone())
        };
        let (r, g, b) = u8_tuple(current);
        let (h, s, v) = rgb_to_hsv(current.r, current.g, current.b);
        let want = [
            (Field::Hex, hex_str(current)),
            (Field::R, r.to_string()),
            (Field::G, g.to_string()),
            (Field::B, b.to_string()),
            (Field::H, format!("{:.0}", h)),
            (Field::S, format!("{:.0}", s * 100.0)),
            (Field::V, format!("{:.0}", v * 100.0)),
        ];
        for (field, text) in want {
            if Some(field) == skip {
                continue;
            }
            let Some(input) = inputs.get(&field) else { continue };
            let focused = input.focus_handle(cx).is_focused(window);
            let same = input.read(cx).value() == text;
            // Never clobber the focused box mid-typing; never rewrite
            // identical text (cursor jumps + Change feedback loops).
            if focused || same {
                continue;
            }
            input.update(cx, |input, cx| {
                input.set_value(&text, window, cx);
            });
        }
    }

    /// Map a pointer position to a color through a control's recorded
    /// bounds, then commit when it differs.
    fn drag_to(
        editor: &Entity<Self>,
        ctl: DragCtl,
        pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (bounds, current) = {
            let this = editor.read(cx);
            (this.bounds_for(ctl), *this.current.borrow())
        };
        let Some(b) = bounds else { return };
        let (bw, bh) = (f32::from(b.size.width), f32::from(b.size.height));
        if bw <= 0.0 || bh <= 0.0 {
            return;
        }
        let tx = ((f32::from(pos.x) - f32::from(b.origin.x)) / bw).clamp(0.0, 1.0);
        let ty = ((f32::from(pos.y) - f32::from(b.origin.y)) / bh).clamp(0.0, 1.0);
        let (r, g, bl) = (current.r, current.g, current.b);
        let (h, s, v) = rgb_to_hsv(r, g, bl);
        let next = match ctl {
            DragCtl::Sv => {
                let (nr, ng, nb) = hsv_to_rgb(h, tx, 1.0 - ty);
                Color::rgba(nr, ng, nb, current.a)
            }
            DragCtl::Hue => {
                let (nr, ng, nb) = hsv_to_rgb(tx * 360.0, s, v);
                Color::rgba(nr, ng, nb, current.a)
            }
            DragCtl::Alpha => Color::rgba(r, g, bl, tx),
            DragCtl::R => Color::rgba(tx, g, bl, current.a),
            DragCtl::G => Color::rgba(r, tx, bl, current.a),
            DragCtl::B => Color::rgba(r, g, tx, current.a),
        };
        Self::commit_synced(editor, next, None, window, cx);
    }

    fn begin_drag(
        editor: &Entity<Self>,
        ctl: DragCtl,
        pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        editor.update(cx, |this, cx| {
            this.dragging = Some(ctl);
            cx.notify();
        });
        Self::drag_to(editor, ctl, pos, window, cx);
    }

    fn end_drag(editor: &Entity<Self>, cx: &mut App) {
        editor.update(cx, |this, cx| {
            if this.dragging.take().is_some() {
                cx.notify();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_roundtrip_primary_and_gray() {
        for (r, g, b) in [
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, 1.0),
            (1.0, 1.0, 0.0),
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            (0.5, 0.5, 0.5),
            (1.0, 0.5, 0.0),
        ] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            for (a, c) in [(r, r2), (g, g2), (b, b2)] {
                assert!((a - c).abs() < 1e-5, "roundtrip {r},{g},{b} -> {h},{s},{v} -> {r2},{g2},{b2}");
            }
        }
        // Known hue anchors.
        assert!((rgb_to_hsv(1.0, 0.0, 0.0).0 - 0.0).abs() < 1e-4);
        assert!((rgb_to_hsv(0.0, 1.0, 0.0).0 - 120.0).abs() < 1e-4);
        assert!((rgb_to_hsv(0.0, 0.0, 1.0).0 - 240.0).abs() < 1e-4);
    }

    #[test]
    fn strict_u8_parse_rejects_partial_and_overflow() {
        assert_eq!(parse_u8("0"), Some(0));
        assert_eq!(parse_u8("255"), Some(255));
        assert_eq!(parse_u8(" 42 "), Some(42));
        assert_eq!(parse_u8(""), None);
        assert_eq!(parse_u8("256"), None);
        assert_eq!(parse_u8("-1"), None);
        assert_eq!(parse_u8("25.5"), None);
        assert_eq!(parse_u8("abc"), None);
    }
}
