//! Automatic internal widgets.
//!
//! The counterpart to `project::widget`: properties declare their UI, these
//! constructors build it. Nothing here knows about individual effects —
//! every renderer takes plain data (ids, labels, values, ranges) plus small
//! commit closures, so new parameters and new effects get full UI for free:
//!
//! ```text
//! PropDecl (type + value + metadata)  →  widget_*()  →  AnyElement
//! ```
//!
//! Commit paths reuse the existing scrub/key/color infrastructure, so undo,
//! keyframing, and typed entry behave identically to the old hand-built rows.

use crate::panels::{self, InspectorColorPicker, PropertiesPanel};
use crate::state::EditorState;
use gpui_kit::base::{h_flex, v_flex, ElementExt as _, TestSupportExt};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::combobox::{Combobox, ComboboxState};
use gpui_kit::*;
use project::{Color, FillGradient, GradientStop, GradientType, PropDecl};
use std::collections::HashMap;

/// Which commit path a scalar row uses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarCommit {
    /// Classic effect param (`fx:` keys, `nudge_effect_param`, keyframes).
    Effect,
    /// Shader Lab param (`sl:` keys, `nudge_shaderlab_param`, no keys).
    ShaderLab,
}

/// Scalar row (Slider / Number / Integer / Percentage / Angle): stopwatch +
/// label on the left, drag-scrub + typed entry on the right. Renders exactly
/// like the historic per-effect rows (same ids, keys, steps, formats).
#[allow(clippy::too_many_arguments)]
pub(crate) fn widget_scalar(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    layer_id: &str,
    eff_id: &str,
    field: &str,
    label: &str,
    display: String,
    step: f32,
    mult100: f32,
    animated: bool,
    commit: ScalarCommit,
    cx: &App,
) -> AnyElement {
    let s_m = state.clone();
    let s_p = state.clone();
    let id_m = eff_id.to_string();
    let id_p = eff_id.to_string();
    let field_m = field.to_string();
    let field_p = field.to_string();
    let mult = format!("{mult100:.0}");
    let (id, key): (SharedString, String) = match commit {
        ScalarCommit::Effect => (
            SharedString::from(format!("param_{field}_{eff_id}")),
            format!("fx:{eff_id}:{field}:{mult}"),
        ),
        ScalarCommit::ShaderLab => (
            SharedString::from(format!("shader_param_{eff_id}_{field}")),
            format!("sl:{eff_id}:{field}"),
        ),
    };
    let label_row = match commit {
        ScalarCommit::Effect => h_flex()
            .gap_1()
            .items_center()
            .child(panels::effect_param_keyframe_controls(
                state, layer_id, eff_id, field, animated, cx,
            ))
            .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
            .into_any_element(),
        ScalarCommit::ShaderLab => div()
            .text_color(cx.theme().muted_foreground)
            .child(label.to_string())
            .into_any_element(),
    };
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(label_row)
        .child(panels::scrub_field(
            id,
            key,
            display,
            None,
            None,
            state,
            panel,
            cx,
            move |cx| {
                let (id_m, field_m) = (id_m.clone(), field_m.clone());
                s_m.update(cx, |s, cx| {
                    match commit {
                        ScalarCommit::Effect => {
                            let _ = s.nudge_effect_param(&id_m, &field_m, -step);
                        }
                        ScalarCommit::ShaderLab => {
                            let _ = s.nudge_shaderlab_param(&id_m, &field_m, -step);
                        }
                    }
                    cx.notify();
                })
            },
            move |cx| {
                let (id_p, field_p) = (id_p.clone(), field_p.clone());
                s_p.update(cx, |s, cx| {
                    match commit {
                        ScalarCommit::Effect => {
                            let _ = s.nudge_effect_param(&id_p, &field_p, step);
                        }
                        ScalarCommit::ShaderLab => {
                            let _ = s.nudge_shaderlab_param(&id_p, &field_p, step);
                        }
                    }
                    cx.notify();
                })
            },
        ))
        .into_any_element()
}

/// Boolean checkbox: kit semantic Checkbox (checked state, tooltip,
/// keyboard + accessibility contract included). Commits through `on_toggle`.
/// `controls` optionally prepends keyframe stopwatch/nav (effect bools).
pub(crate) fn widget_bool<F>(
    label: &str,
    on: bool,
    test_id: String,
    tooltip: &str,
    on_toggle: F,
    controls: Option<AnyElement>,
    cx: &App,
) -> AnyElement
where
    F: Fn(bool, &mut App) + 'static,
{
    let label_cell = if let Some(ctl) = controls {
        h_flex()
            .gap_1()
            .items_center()
            .child(ctl)
            .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
            .into_any_element()
    } else {
        div().text_color(cx.theme().muted_foreground).child(label.to_string()).into_any_element()
    };
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .py_0p5()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(label_cell)
        .child(
            div()
                .id(SharedString::from(test_id.clone()))
                .test_support()
                .child(
                    Checkbox::new(SharedString::from(format!("{test_id}_box")))
                        .checked(on)
                        .label(if on { "On" } else { "Off" })
                        .tooltip(tooltip.to_string())
                        .on_click(move |checked: &bool, _window, cx| {
                            on_toggle(*checked, cx);
                        }),
                ),
        )
        .into_any_element()
}

/// Enum dropdown: kit Combobox (searchable, keyboard nav, dismissal and
/// a11y included). The retained state entity is provisioned by the
/// Properties panel; Confirm commits through the shader setter.
pub(crate) fn widget_dropdown(
    eff_id: &str,
    field: &str,
    label: &str,
    combo: Option<&Entity<ComboboxState<SearchableVec<String>>>>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
        .child(match combo {
            Some(st) => div()
                .id(SharedString::from(format!("shader_enum_{eff_id}_{field}")))
                .test_support()
                .child(
                    Combobox::new(st)
                        .placeholder(label.to_string()),
                )
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("—")
                .into_any_element(),
        })
        .into_any_element()
}

/// Vector row (Vec2/Vec3/Vec4): per-component scrub rows with X/Y/Z/W tags
/// plus a Link pill (linked scrubs move every component together).
#[allow(clippy::too_many_arguments)]
pub(crate) fn widget_vec(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    eff_id: &str,
    field: &str,
    label: &str,
    tags: &[&str],
    values: &[f32],
    linked: bool,
    cx: &App,
) -> AnyElement {
    let link_key = format!("{eff_id}:{field}");
    let p_link = panel.clone();
    let link_key_t = link_key.clone();
    let mut col = v_flex().gap_0p5();
    col = col.child(
        h_flex()
            .items_center()
            .justify_between()
            .text_xs()
            .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
            .child(
                div()
                    .id(SharedString::from(format!("vec_link_{eff_id}_{field}")))
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(if linked { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if linked {
                        cx.theme().primary_foreground
                    } else {
                        cx.theme().foreground
                    })
                    .hover(|s| s.opacity(0.85))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_link.update(cx, |this, cx| {
                            if linked {
                                this.vec_link.remove(&link_key_t);
                            } else {
                                this.vec_link.insert(link_key_t.clone());
                            }
                            cx.notify();
                        });
                    })
                    .child(if linked { "Linked" } else { "Link" }),
            ),
    );
    for (i, (tag, v)) in tags.iter().zip(values.iter()).enumerate() {
        let key = if linked {
            format!("slcl:{eff_id}:{field}")
        } else {
            format!("slc:{eff_id}:{field}:{i}")
        };
        col = col.child(
            h_flex()
                .items_center()
                .justify_between()
                .text_xs()
                .child(
                    div()
                        .w(px(52.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("  {tag}")),
                )
                .child(panels::scrub_field(
                    SharedString::from(format!("shader_param_{eff_id}_{field}_{i}")),
                    key,
                    format!("{v:.3}"),
                    None,
                    None,
                    state,
                    panel,
                    cx,
                    move |_| {},
                    move |_| {},
                )),
        );
    }
    col.into_any_element()
}

/// Shared N-stop gradient editor (Gradient Ramp ramps, text/shape/solid
/// fill slots): preview bar, stop diamonds, selected-stop color, reverse +
/// presets, quick swatches.
///
/// Interaction (After Effects-style):
/// - click an empty bar stretch: add a stop there (sampled color) and
///   select it;
/// - left-press a diamond: select it and start a drag (panel-root mouse
///   handlers commit offsets; release ends the gesture — one undo step);
/// - right-click a diamond: delete it (minimum two stops);
/// - the bar ignores the bubbled press of a diamond grab (the diamond arms
///   `gradient_drag` first).
///
/// Commits route through the target's state methods; `stop_color_editor` is
/// caller-built (effect endpoint wheel, fill wheel/palette, …).
pub(crate) fn gradient_editor(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    target: panels::GradientTarget,
    stops: Vec<GradientStop>,
    selected: usize,
    stop_color_editor: AnyElement,
    cx: &App,
) -> AnyElement {
    fn to_rgba(c: Color) -> Rgba {
        Rgba { r: c.r, g: c.g, b: c.b, a: 1.0 }
    }
    let prefix = target.id_prefix();
    let sel = selected.min(stops.len().saturating_sub(1));
    let sel_color = stops.get(sel).map(|s| s.color).unwrap_or(Color::WHITE);

    // N-stop preview bar (GPUI has no gradient fills): sample the stops.
    let probe = FillGradient { stops: stops.clone(), angle: 0.0, gradient_type: project::GradientType::Linear };
    let mut bar = h_flex()
        .flex_1()
        .h(px(22.))
        .rounded_sm()
        .overflow_hidden()
        .cursor_pointer();
    for i in 0..24 {
        let c = probe.sample(i as f32 / 23.0);
        bar = bar.child(div().flex_1().h_full().bg(to_rgba(c)));
    }
    {
        let p_prep = panel.clone();
        let p_down = panel.clone();
        let s_add = state.clone();
        let prefix_prep = prefix.clone();
        let prefix_down = prefix.clone();
        let target_add = target.clone();
        let stops_add = stops.clone();
        bar = bar
            .on_prepaint(move |bounds, _window, cx| {
                let ox = bounds.origin.x / px(1.0);
                let w = bounds.size.width / px(1.0);
                p_prep.update(cx, |this, cx| {
                    let key = format!("gradient_bar_{prefix_prep}");
                    if this.gradient_bar_bounds.get(&key) != Some(&(ox, w)) {
                        this.gradient_bar_bounds.insert(key, (ox, w));
                        cx.notify();
                    }
                });
            })
            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                // Diamond grabs bubble here; the diamond arms gradient_drag
                // first, so an armed drag means "not a bar click".
                let armed = p_down.read(cx).gradient_drag.clone().map(|d| d.target)
                    == Some(target_add.clone());
                if armed {
                    return;
                }
                let mx = event.position.x / px(1.0);
                let (ox, w) = p_down
                    .read(cx)
                    .gradient_bar_bounds
                    .get(&format!("gradient_bar_{prefix_down}"))
                    .copied()
                    .unwrap_or((mx, 100.0));
                let t = ((mx - ox) / w.max(1.0)).clamp(0.0, 1.0);
                let probe = FillGradient { stops: stops_add.clone(), angle: 0.0, gradient_type: project::GradientType::Linear };
                let color = probe.sample(t);
                let target_do = target_add.clone();
                let at = s_add.update(cx, |s, _| match &target_do {
                    panels::GradientTarget::Effect { layer_id, eff_id } => {
                        s.add_effect_gradient_stop(layer_id, eff_id, t, color)
                    }
                    panels::GradientTarget::Fill { layer_id, key } => {
                        s.add_fill_gradient_stop(layer_id, key, t, color)
                    }
                });
                if let Ok(at) = at {
                    p_down.update(cx, |this, cx| {
                        match &target_add {
                            panels::GradientTarget::Effect { eff_id, .. } => {
                                this.gradient_stop.insert(eff_id.clone(), at);
                            }
                            panels::GradientTarget::Fill { key, .. } => {
                                this.color_picker_gradient_stop.insert(key.clone(), at);
                            }
                        }
                        cx.notify();
                    });
                    s_add.update(cx, |_, cx| cx.notify());
                }
            });
    }
    let bar = div()
        .id(SharedString::from(format!("gradient_bar_{prefix}")))
        .test_support()
        .relative()
        .child(bar);
    let mut wrap = div().relative().child(bar);
    // Movable stop diamonds, centered on their offsets.
    for (i, stop) in stops.iter().enumerate() {
        let stop_offset = stop.offset;
        let p_sel = panel.clone();
        let s_down = state.clone();
        let s_del = state.clone();
        let p_del = panel.clone();
        let target_sel = target.clone();
        let target_del = target.clone();
        let stop_id = match &target {
            panels::GradientTarget::Effect { eff_id, .. } if i < 2 => {
                format!("gradient_stop{i}_{eff_id}")
            }
            _ => format!("gradient_stop_{prefix}_{i}"),
        };
        wrap = wrap.child(
            div()
                .id(SharedString::from(stop_id))
                .test_support()
                .absolute()
                .left(relative(stop.offset.clamp(0.0, 1.0)))
                .top(px(-4.))
                .ml(px(-6.))
                .cursor_grab()
                .w(px(12.))
                .h(px(12.))
                .rounded_sm()
                .bg(to_rgba(stop.color))
                .border_1()
                .border_color(if i == sel { rgb(0xffffff) } else { rgb(0x000000) })
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    s_down.update(cx, |s, cx| {
                        s.checkpoint();
                        cx.notify();
                    });
                    p_sel.update(cx, |this, cx| {
                        match &target_sel {
                            panels::GradientTarget::Effect { eff_id, .. } => {
                                this.gradient_stop.insert(eff_id.clone(), i);
                            }
                            panels::GradientTarget::Fill { key, .. } => {
                                this.color_picker_gradient_stop.insert(key.clone(), i);
                            }
                        }
                        this.gradient_drag = Some(panels::GradientDrag {
                            target: target_sel.clone(),
                            index: i,
                            moved: false,
                            last_t: stop_offset,
                        });
                        cx.notify();
                    });
                })
                .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                    let target_do = target_del.clone();
                    let res = s_del.update(cx, |s, _| match &target_do {
                        panels::GradientTarget::Effect { layer_id, eff_id } => {
                            s.remove_effect_gradient_stop(layer_id, eff_id, i)
                        }
                        panels::GradientTarget::Fill { layer_id, key } => {
                            s.remove_fill_gradient_stop(layer_id, key, i)
                        }
                    });
                    if res.is_ok() {
                        p_del.update(cx, |_, cx| cx.notify());
                        s_del.update(cx, |_, cx| cx.notify());
                    }
                }),
        );
    }
    let cur_type = match &target {
        panels::GradientTarget::Effect { layer_id, eff_id } => {
            state.read(cx).effect_gradient_type(layer_id, eff_id).unwrap_or_default()
        }
        panels::GradientTarget::Fill { layer_id, key } => {
            state.read(cx).layer_fill_gradient(layer_id, key).map(|g| g.gradient_type).unwrap_or_default()
        }
    };
    let mut type_row = h_flex().gap_1().items_center().text_xs().mb_1();
    type_row = type_row.child(div().text_color(cx.theme().muted_foreground).child("Type:"));
    for g_type in GradientType::ALL {
        let is_active = g_type == cur_type;
        let s_typ = state.clone();
        let target_typ = target.clone();
        let label = g_type.label();
        type_row = type_row.child(
            div()
                .id(SharedString::from(format!("grad_type_{prefix}_{label}")))
                .test_support()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(if is_active { cx.theme().accent } else { cx.theme().muted })
                .text_color(if is_active { cx.theme().accent_foreground } else { cx.theme().foreground })
                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                .child(label)
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    let res = s_typ.update(cx, |s, _| match &target_typ {
                        panels::GradientTarget::Effect { layer_id, eff_id } => {
                            s.set_effect_gradient_type(layer_id, eff_id, g_type)
                        }
                        panels::GradientTarget::Fill { layer_id, key } => {
                            s.set_fill_gradient_type(layer_id, key, g_type)
                        }
                    });
                    if res.is_ok() {
                        s_typ.update(cx, |_, cx| cx.notify());
                    }
                }),
        );
    }
    let mut col = v_flex().gap_1().child(type_row).child(wrap);
    // Selected stop: chip + hex + caller-built color editor.
    {
        let hex = format!(
            "#{:02X}{:02X}{:02X}",
            (sel_color.r * 255.0).round() as u8,
            (sel_color.g * 255.0).round() as u8,
            (sel_color.b * 255.0).round() as u8
        );
        col = col.child(
            h_flex()
                .items_center()
                .justify_between()
                .text_xs()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(div().w(px(20.)).h(px(14.)).rounded_sm().bg(to_rgba(sel_color)))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Stop {} · {hex}", sel + 1)),
                        ),
                )
                .child(stop_color_editor),
        );
    }
    // Reverse + presets (single-undo whole-gradient commits).
    {
        let presets: [(&str, Color, Color); 4] = [
            ("B/W", Color::BLACK, Color::WHITE),
            ("Fire", Color::from_hex("#7C2D12").unwrap(), Color::from_hex("#FDE68A").unwrap()),
            ("Ocean", Color::from_hex("#082F49").unwrap(), Color::from_hex("#7DD3FC").unwrap()),
            ("Sunset", Color::from_hex("#4C1D95").unwrap(), Color::from_hex("#FB7185").unwrap()),
        ];
        let mut row = h_flex().gap_1().items_center().text_xs();
        {
            let s_rev = state.clone();
            let target_rev = target.clone();
            let prefix_rev = prefix.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("gradient_reverse_{prefix_rev}")))
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        let target_do = target_rev.clone();
                        let res = s_rev.update(cx, |s, _| match &target_do {
                            panels::GradientTarget::Effect { layer_id, eff_id } => {
                                s.reverse_effect_gradient(layer_id, eff_id)
                            }
                            panels::GradientTarget::Fill { layer_id, key } => {
                                s.reverse_fill_gradient(layer_id, key)
                            }
                        });
                        if res.is_ok() {
                            s_rev.update(cx, |_, cx| cx.notify());
                        }
                    })
                    .child("Reverse"),
            );
        }
        for (name, pa, pb) in presets {
            let s_pre = state.clone();
            let target_pre = target.clone();
            let prefix_pre = prefix.clone();
            let name_owned = name.to_string();
            row = row.child(
                div()
                    .id(SharedString::from(format!("gradient_preset_{prefix_pre}_{name_owned}")))
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        let target_do = target_pre.clone();
                        let grad = FillGradient::two_color(pa, pb, 0.0);
                        let res = s_pre.update(cx, |s, _| match &target_do {
                            panels::GradientTarget::Effect { layer_id, eff_id } => {
                                s.set_effect_gradient_stops(layer_id, eff_id, grad.stops.clone())
                            }
                            panels::GradientTarget::Fill { layer_id, key } => {
                                let mut grad = grad.clone();
                                // Keep the fill's current axis; presets swap colors.
                                if let Some(cur) = s.layer_fill_gradient(layer_id, key) {
                                    grad.angle = cur.angle;
                                }
                                s.set_layer_fill_gradient(layer_id, key, Some(grad))
                            }
                        });
                        if res.is_ok() {
                            s_pre.update(cx, |_, cx| cx.notify());
                        }
                    })
                    .child(name_owned),
            );
        }
        col = col.child(row);
    }
    col.into_any_element()
}

/// Gradient Ramp effect adapter: endpoint wheels for the outer stops
/// (middle stops recolor through the quick swatches), selection from the
/// panel's per-effect map.
#[allow(clippy::too_many_arguments)]
pub(crate) fn widget_gradient(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    layer_id: &str,
    eff_id: &str,
    stops: Vec<GradientStop>,
    sel: usize,
    wheels: &HashMap<(String, String), Entity<InspectorColorPicker>>,
    cx: &App,
) -> AnyElement {
    let field = if sel == 0 {
        Some("color_a")
    } else if sel + 1 >= stops.len().max(1) {
        Some("color_b")
    } else {
        None
    };
    let stop_color = stops.get(sel).map(|s| s.color).unwrap_or(Color::WHITE);
    let stop_editor: AnyElement = match field {
        Some(f) => panels::fx_swatch_row(
            state,
            layer_id,
            eff_id,
            f,
            "Stop",
            stop_color,
            wheels.get(&(eff_id.to_string(), f.to_string())),
            cx,
        ),
        None => div().into_any_element(),
    };
    gradient_editor(
        state,
        panel,
        panels::GradientTarget::Effect {
            layer_id: layer_id.to_string(),
            eff_id: eff_id.to_string(),
        },
        stops,
        sel,
        stop_editor,
        cx,
    )
}

/// Layer fill-slot adapter (text/sharp/solid picker gradient tabs).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_gradient_editor(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    layer_id: &str,
    key: &str,
    gradient: &FillGradient,
    sel: usize,
    stop_color_editor: AnyElement,
    cx: &App,
) -> AnyElement {
    gradient_editor(
        state,
        panel,
        panels::GradientTarget::Fill {
            layer_id: layer_id.to_string(),
            key: key.to_string(),
        },
        gradient.stops.clone(),
        sel,
        stop_color_editor,
        cx,
    )
}

/// Dispatch one declaration to its widget (the automatic UI).
#[allow(clippy::too_many_arguments)]
pub(crate) fn widget_for_decl(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    layer_id: &str,
    eff_id: &str,
    decl: &PropDecl,
    wheels: &HashMap<(String, String), Entity<InspectorColorPicker>>,
    enums: &HashMap<(String, String), Entity<ComboboxState<SearchableVec<String>>>>,
    cx: &App,
) -> AnyElement {
    use project::PropValue;
    use project::WidgetKind;
    match decl.widget {
        WidgetKind::Slider
        | WidgetKind::Number
        | WidgetKind::Integer
        | WidgetKind::Percentage
        | WidgetKind::Angle => match decl.value {
            PropValue::Float { value, animated } => widget_scalar(
                state,
                panel,
                layer_id,
                eff_id,
                &decl.field,
                &decl.label,
                decl.meta.display(value),
                decl.meta.step,
                decl.meta.mult100,
                animated,
                ScalarCommit::Effect,
                cx,
            ),
            _ => div().into_any_element(),
        },
        WidgetKind::Color => match decl.value {
            PropValue::Color { value: c, animated } => h_flex()
                .gap_1()
                .items_center()
                .child(panels::effect_param_keyframe_controls(
                    state, layer_id, eff_id, &decl.field, animated, cx,
                ))
                .child(panels::fx_swatch_row(
                    state,
                    layer_id,
                    eff_id,
                    &decl.field,
                    &decl.label,
                    c,
                    wheels.get(&(eff_id.to_string(), decl.field.clone())),
                    cx,
                ))
                .into_any_element(),
            _ => div().into_any_element(),
        },
        WidgetKind::Checkbox => match decl.value {
            PropValue::Bool { value: on, animated } => {
                let s_t = state.clone();
                let lid = layer_id.to_string();
                let eid = eff_id.to_string();
                let fld = decl.field.clone();
                let ctl = panels::effect_param_keyframe_controls(
                    state, layer_id, eff_id, &decl.field, animated, cx,
                );
                widget_bool(
                    &decl.label,
                    on,
                    format!("fx_bool_{}_{}", decl.field, eff_id),
                    &format!("Toggle {}", decl.label),
                    move |is_on: bool, cx: &mut App| {
                        let (lid, eid, fld) = (lid.clone(), eid.clone(), fld.clone());
                        s_t.update(cx, |s, cx| {
                            if fld == "monochrome" {
                                let _ = s.toggle_noise_monochrome(&eid);
                            } else {
                                let _ = s.set_effect_bool(&lid, &eid, &fld, is_on);
                            }
                            cx.notify();
                        });
                    },
                    Some(ctl),
                    cx,
                )
            }
            _ => div().into_any_element(),
        },
        WidgetKind::Dropdown => match &decl.value {
            PropValue::EnumSel { index } => {
                let options = decl.meta.options.clone();
                let label = decl.label.clone();
                let combo_el: AnyElement =
                    match enums.get(&(eff_id.to_string(), decl.field.clone())) {
                        Some(st) => div()
                            .id(SharedString::from(format!(
                                "fx_enum_{}_{}",
                                eff_id, decl.field
                            )))
                            .test_support()
                            .child(
                                Combobox::new(st).placeholder(label.clone()),
                            )
                            .into_any_element(),
                        None => div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                options
                                    .get(*index)
                                    .cloned()
                                    .unwrap_or_else(|| "—".to_string()),
                            )
                            .into_any_element(),
                    };
                h_flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(combo_el)
                    .into_any_element()
            }
            _ => div().into_any_element(),
        },
        // Gradient / Curve / Path / Text / Time / Toggle / Radio /
        // Searchable / XYPad need richer contexts; the panel composes them
        // explicitly (gradient pairs, graph tangents, mask paths, …).
        _ => div().into_any_element(),
    }
}
