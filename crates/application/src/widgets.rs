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
use gpui_kit::base::{h_flex, v_flex, TestSupportExt};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::*;
use project::{Color, PropDecl};
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
pub(crate) fn widget_bool<F>(
    label: &str,
    on: bool,
    test_id: String,
    tooltip: &str,
    on_toggle: F,
    cx: &App,
) -> AnyElement
where
    F: Fn(bool, &mut App) + 'static,
{
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .py_0p5()
        .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
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

/// Enum dropdown: kit Select (keyboard nav, search, dismissal and a11y
/// included). The retained state entity is provisioned by the Properties
/// panel; Confirm commits through the shader setter.
pub(crate) fn widget_dropdown(
    eff_id: &str,
    field: &str,
    label: &str,
    select: Option<&Entity<SelectState<SearchableVec<String>>>>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
        .child(match select {
            Some(st) => div()
                .id(SharedString::from(format!("shader_enum_{eff_id}_{field}")))
                .test_support()
                .child(
                    Select::new(st)
                        .accessibility_label(format!("{label} values"))
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

/// Two-stop gradient editor (Tint maps, GradientRamp start/end): stop bar
/// with selectable stops, wheel + hex for the selected stop, reverse and
/// presets. Commits through the same color setters as the swatch rows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn widget_gradient(
    state: &Entity<EditorState>,
    panel: &Entity<PropertiesPanel>,
    layer_id: &str,
    eff_id: &str,
    stop_a: (&str, &str, Color),
    stop_b: (&str, &str, Color),
    sel: usize,
    wheels: &HashMap<(String, String), Entity<InspectorColorPicker>>,
    cx: &App,
) -> AnyElement {
    fn to_rgba(c: Color) -> Rgba {
        Rgba { r: c.r, g: c.g, b: c.b, a: 1.0 }
    }
    let (fa, la, ca) = (stop_a.0.to_string(), stop_a.1.to_string(), stop_a.2);
    let (fb, lb, cb) = (stop_b.0.to_string(), stop_b.1.to_string(), stop_b.2);
    let (sel_field, sel_label, sel_color) = if sel == 0 {
        (fa.clone(), la.clone(), ca)
    } else {
        (fb.clone(), lb.clone(), cb)
    };
    // 24-segment preview bar (GPUI has no gradient fills).
    let mut bar = h_flex().flex_1().h(px(22.)).rounded_sm().overflow_hidden();
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
    let mut col = v_flex().gap_1();
    // Stop bar with selectable diamonds.
    {
        let p_sa = panel.clone();
        let p_sb = panel.clone();
        let eid_a = eff_id.to_string();
        let eid_b = eff_id.to_string();
        col = col.child(
            div()
                .relative()
                .child(bar)
                .child(
                    div()
                        .id(SharedString::from(format!("gradient_stop0_{eff_id}")))
                        .test_support()
                        .absolute()
                        .left(px(2.))
                        .top(px(-3.))
                        .cursor_pointer()
                        .w(px(12.))
                        .h(px(12.))
                        .rounded_sm()
                        .bg(to_rgba(ca))
                        .border_1()
                        .border_color(if sel == 0 { rgb(0xffffff) } else { rgb(0x000000) })
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_sa.update(cx, |this, cx| {
                                this.gradient_stop.insert(eid_a.clone(), 0);
                                cx.notify();
                            });
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("gradient_stop1_{eff_id}")))
                        .test_support()
                        .absolute()
                        .right(px(2.))
                        .top(px(-3.))
                        .cursor_pointer()
                        .w(px(12.))
                        .h(px(12.))
                        .rounded_sm()
                        .bg(to_rgba(cb))
                        .border_1()
                        .border_color(if sel == 1 { rgb(0xffffff) } else { rgb(0x000000) })
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_sb.update(cx, |this, cx| {
                                this.gradient_stop.insert(eid_b.clone(), 1);
                                cx.notify();
                            });
                        }),
                ),
        );
    }
    // Selected stop: wheel (same ids as the swatch rows) + hex.
    {
        let wheel_el: AnyElement = match wheels.get(&(eff_id.to_string(), sel_field.clone())) {
            Some(picker) => panels::fx_wheel_el(&sel_field, eff_id, picker, cx),
            None => div().into_any_element(),
        };
        let hex = format!(
            "#{:02X}{:02X}{:02X}",
            (sel_color.r * 255.0) as u8,
            (sel_color.g * 255.0) as u8,
            (sel_color.b * 255.0) as u8
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
                        .child(div().text_color(cx.theme().muted_foreground).child(format!("{sel_label} · {hex}"))),
                )
                .child(wheel_el),
        );
    }
    // Reverse + presets (single-undo pair commits).
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
            let lid = layer_id.to_string();
            let eid = eff_id.to_string();
            let (fa_r, fb_r) = (fa.clone(), fb.clone());
            row = row.child(
                div()
                    .id(SharedString::from(format!("gradient_reverse_{eff_id}")))
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_rev.update(cx, |s, cx| {
                            let _ = s.set_effect_color_pair(&lid, &eid, &[(&fa_r, cb), (&fb_r, ca)]);
                            cx.notify();
                        });
                    })
                    .child("Reverse"),
            );
        }
        for (name, pa, pb) in presets {
            let s_pre = state.clone();
            let lid = layer_id.to_string();
            let eid = eff_id.to_string();
            let (fa_p, fb_p) = (fa.clone(), fb.clone());
            row = row.child(
                div()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_pre.update(cx, |s, cx| {
                            let _ = s.set_effect_color_pair(&lid, &eid, &[(&fa_p, pa), (&fb_p, pb)]);
                            cx.notify();
                        });
                    })
                    .child(name.to_string()),
            );
        }
        col = col.child(row);
    }
    // Stop color swatches (compact presets for the selected stop).
    {
        let mut sprow = h_flex().gap_1().items_center();
        for (hex_str, col_val) in [
            ("#FFFFFF", Color::WHITE),
            ("#000000", Color::BLACK),
            ("#EF4444", Color::from_hex("#EF4444").unwrap()),
            ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
            ("#10B981", Color::from_hex("#10B981").unwrap()),
            ("#3B82F6", Color::from_hex("#3B82F6").unwrap()),
        ] {
            let s_p = state.clone();
            let lid_p = layer_id.to_string();
            let eid_p = eff_id.to_string();
            let fld = sel_field.clone();
            sprow = sprow.child(
                div()
                    .cursor_pointer()
                    .w(px(14.))
                    .h(px(14.))
                    .rounded_sm()
                    .bg(to_rgba(col_val))
                    .border_1()
                    .border_color(cx.theme().border)
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_p.update(cx, |s, cx| {
                            let _ = s.set_effect_color(&lid_p, &eid_p, &fld, col_val);
                            cx.notify();
                        });
                    }),
            );
            let _ = hex_str;
        }
        col = col.child(sprow);
    }
    col.into_any_element()
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
            PropValue::Color(c) => panels::fx_swatch_row(
                state,
                layer_id,
                eff_id,
                &decl.field,
                &decl.label,
                c,
                wheels.get(&(eff_id.to_string(), decl.field.clone())),
                cx,
            ),
            _ => div().into_any_element(),
        },
        WidgetKind::Checkbox => match decl.value {
            PropValue::Bool(on) => {
                let s_t = state.clone();
                let eid = eff_id.to_string();
                let fld = decl.field.clone();
                widget_bool(
                    &decl.label,
                    on,
                    format!("fx_bool_{}_{}", decl.field, eff_id),
                    &format!("Toggle {}", decl.label),
                    move |_on: bool, cx: &mut App| {
                        let (eid, fld) = (eid.clone(), fld.clone());
                        s_t.update(cx, |s, cx| {
                            // Classic bool params toggle through their own
                            // setters (only monochrome exists today).
                            if fld == "monochrome" {
                                let _ = s.toggle_noise_monochrome(&eid);
                            }
                            cx.notify();
                        });
                    },
                    cx,
                )
            }
            _ => div().into_any_element(),
        },
        // Gradient / Curve / Path / Text / Time / Toggle / Radio /
        // Searchable / XYPad need richer contexts; the panel composes them
        // explicitly (gradient pairs, graph tangents, mask paths, …).
        _ => div().into_any_element(),
    }
}
