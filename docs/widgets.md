# Reusable Widgets & UI Elements

Single source of truth for the shared UI building blocks. **Reuse these
instead of building new controls** — every item below already handles
scrub/drag, keyboard entry, keyframes, undo, test ids, and monochrome
theming. All live in `crates/application/src/` (`widgets.rs`,
`panels.rs`, `color_editor.rs`) unless noted.

Conventions every widget follows:
- Every interactive element gets `.id(...)` + `.test_support()`.
- Value color: `cx.theme().foreground`; labels: `muted_foreground`; rows:
  `text_xs` + `border_b_1` + `ae::border()`.
- No hue in chrome (black/white/gray only); hue lives in content
  (swatches, canvas, gradient bars).
- Scrub keys must exist in **both** `set_scrub_value` (typed entry) and
  `apply_scrub_delta` (drag) in `state.rs`/`panels.rs`, or one of the two
  silently does nothing.
- Discrete commits checkpoint (undo step); drag/move packets never
  checkpoint — the gesture owner checkpoints once on press.

## Scalar rows

- `widget_scalar(state, panel, layer_id, eff_id, field, label, display,
  step, mult100, animated, commit, id_prefix, cx)` (`widgets.rs`) — stopwatch
  + label left, drag-scrub + typed entry right. `commit: ScalarCommit::Effect`
  routes `fx:{eff}:{field}` keys + `nudge_effect_param`; `ShaderLab` routes
  `sl:` keys. `id_prefix` disambiguates repeat renderings of one param
  (e.g. `"stroke_"` for trim rows mirrored in the shape STROKE section).
- `widget_for_decl(...)` (`widgets.rs`) — dispatches one `PropDecl`
  (Slider/Number/Integer/Percentage/Angle/Color/Checkbox/Vec/...) to the
  right widget. **New effect params need no UI code**: add the declaration
  in `EffectType::declarations()` and this renders it.
- `scrub_field(id, prop_key, label, .., state, panel, cx, on_minus,
  on_plus)` (`panels.rs`) — the primitive everything else builds on:
  drag-scrub, wheel steps, click-to-type entry, right-click menu (reset,
  keyframe toggle, `Modifier Graph…`, copy/paste links). Linked/modified
  values auto-switch to the live-value display.

## Bool / enum rows

- `widget_bool(label, on, test_id, tooltip, on_toggle, controls, cx)` —
  label + kit `Checkbox`.
- `widget_dropdown(eff_id, field, label, combo, cx)` — kit `Combobox`
  (search, keyboard nav, a11y). The retained `ComboboxState` entity is
  provisioned by the Properties panel per `(eff_id, field)`; Confirm
  commits through the shader setter.
- `shape_enum_pill(state, layer_id, id, label, current, options, set, cx)`
  (`panels.rs`) — lightweight cycle-on-click alternative (`Butt/Round/
  Square…`): shows the value, click advances. `set` is a plain
  `fn(&mut EditorState, &str, &str)` (e.g.
  `EditorState::set_layer_shape_line_cap`).

## Vector rows

- `widget_vec(state, panel, eff_id, field, label, tags, ...)` — X/Y/Z/W
  scrub rows plus a Link pill (linked scrubs move all components).
- Layer transform Position/Scale/Anchor rows in `panels.rs` are the
  hand-built equivalent (dual scrub fields + stopwatch); mask transform
  rows (`mask_transform_rows`) reuse the same shape.

## Color

- `render_color_swatch(id, color, mode, gradient, is_active, cx,
  on_click)` — three modes: `none` (slashed box), `color`, `gradient`
  (16-slice bar).
- `render_three_mode_color_picker(key, color, panel, ...)` — None | Color
  | Gradient tabs. Tab switches **commit** (Color tab commits opaque
  white, None commits transparent + clears gradient) — this is what keeps
  picks visible.
- `InspectorColorPicker` + `render_color_editor(editor, id, current, cx)`
  (`color_editor.rs`) — RGB/HEX/HSV tabs, SV canvas, hue/alpha/RGB sliders,
  numeric fields. Created once per usage via `use_keyed_state` with a
  `commit` closure (e.g. `shape_stroke` → `set_layer_shape_stroke`).
  Drag gestures checkpoint once in `begin_drag`, suppress per-packet
  checkpoints (`EditorState::suppress_checkpoint`), run the canvas
  degraded (`preview_fast`), and restore both in `end_drag`. Helpers:
  `rgb_to_hsv`, `hsv_to_rgb`, `sync_inputs`.
- `fx_swatch_row(state, layer_id, eff_id, field, label, color, wheel, cx)`
  (`panels.rs`) — effect color row bound to a wheel editor from the
  per-`(eff_id, field)` registry (`fx_color_fields` lists an effect's
  color fields; add new ones there or wheels never materialize).

## Gradients

- `gradient_editor(state, panel, target, stops, selected,
  stop_color_editor, cx)` (`widgets.rs`) — the one shared N-stop editor
  (ramps, text/shape/solid fill slots): 64-slice preview bar, dual-ring
  diamonds (black outer + white-bordered core, selected runs larger),
  click-empty-bar adds a stop, diamond drag moves (one undo step, degraded
  preview), right-click deletes (2-stop minimum).
- `target: GradientTarget::Effect { layer_id, eff_id } | Fill
  { layer_id, key }` — commits route to `move/add/remove_*_gradient_stop`;
  selection lives in panel maps (`gradient_stop` / `color_picker_gradient_stop`).
- `widget_gradient(...)` (ramp adapter: endpoint wheels) and
  `fill_gradient_editor(...)` (fill-slot adapter) wrap it.

## Sections & rows

- `prop_section(id, title, icon, expanded, on_toggle, cx, body)` —
  collapsible card (chevron + `TITLE`). Section state lives as bools on
  `PropertiesPanel` (`source_expanded`, `stroke_section_expanded`, …).
- `nested_group(toggle_id, title, open, on_toggle, content)` — twirl-down
  group inside an open card (applied-effect param groups, STROKE group).
- Stopwatches: `property_stopwatch` (layer props), `effect_param_keyframe_
  controls` (effect params incl. colors), `timeline_stopwatch_nav`
  (timeline variant with prev/next/diamond).
- Param rows: `mask_param_row` (scalar + `mask:` routing), `text_param_row`,
  `shape_fill_row` (shared 3-mode fill for rect/ellipse/path),
  `shape_stroke_rows` (shared stroke color + scrub width),
  `shape_stroke_section` (position/ends/joints pills + trim + dash),
  `mask_transform_rows` (Position/Rotation/Scale).
- Applied-effect cards (`render_applied_effects`) start expanded;
  `fx_collapsed: HashSet<String>` tracks explicit user collapses only.

## Timeline rows

- `timeline_scrub(id, label, display, layer_id, key, drag, step, ...)` —
  lane-embedded scrub (keys route like inspector fields).
- `timeline_stopwatch_nav(...)`, `timeline_keyframe_lane(...)` — diamond
  lanes from keyframe time lists; clicking a diamond seeks.

## Curve (graph) editor

- `render_graph_view(state, panel_entity, selected_layer_id, gui, cx)` +
  `GraphUi { tab, isolate, show_grid, show_keys, hidden, view, drag,
  sel_keys, marquee }` (`panels.rs`) — keyframe value curves with grid,
  marquee selection, key dragging, per-prop isolation.
- State API (`state.rs`): `toggle_spline_editor`, `set_spline_prop_path`,
  `get_spline_keyframe_points`, `get_spline_curve_samples`.

## Modifier graph window

- `open_modifier_graph_window(state, layer_id, prop_path, cx)`
  (`modifier_graph_view.rs`) — node canvas for one bound property.
- Paths are canonical `shape.*` / `transform.*` / `effect:{id}:{param}` /
  `opacity` dialects; `EditorState::canonical_shape_modifier_path`
  folds scrub aliases (`rect_w`, `ellipse_sw`, …) at set/get/remove.
  Engine is f32-only: scalars bind, colors don't.
- Node catalog (`crates/project/src/modifier.rs`, `NodeKind`): sources
  (`BaseValueIn`, `Constant`, `GetLayerFactor`, `DriverLink`), math
  (`Math` +−×÷^, `Lerp`, `Clamp`, `Remap`, `Smoothstep`, `Stepped`),
  generators (`Noise`, `Wave` sine/triangle/square), `Output` with named
  sockets (`input_sockets` / `output_sockets`).

## Adding a new control: checklist

1. Reuse the closest item above; new one-off controls need `.id` +
   `.test_support` + a UI test that finds/clicks them.
2. New scrub key? Add it to `set_scrub_value`, `apply_scrub_delta`,
   `scrub_current_value`, and the stopwatch read/toggle matches
   (`shape.*` / `text.*` / `effect:` arms).
3. Discrete commit → `self.checkpoint()`; drag/move path → no checkpoint
   (gesture owner checkpoints on press).
4. New effect color field? Add to `fx_color_fields` + `set_color_value` +
   `get_color_property[_mut]`.
5. New effect param? Only `declarations()` + model plumbing — no UI code.
