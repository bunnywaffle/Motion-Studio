# Current State

## What Currently Works
- Cargo workspace initialized at repository root (`Cargo.toml`).
- Binary crate created under `crates/application`.
- `gpui-kit` dependency integrated in workspace dependencies and consumed by `crates/application`.
- Application bootstrap in `crates/application/src/main.rs` using `gpui_kit::application()` and `gpui_kit::init(cx)`.
- Dark theme initialized with `Theme::change(ThemeMode::Dark, None, cx)` with components consuming active theme tokens (`cx.theme().background`, `cx.theme().foreground`).
- Native window created via `cx.open_window(WindowOptions::default(), ...)` wrapping root element with `Root::new` and platform title set, calling `window.activate_window()`.
- GPUI Kit's built-in docking system (`DockArea`, `DockSkin`, `DockPlacement`, `DockLayout`, `Panel`, `BasePanel`, `panel_handle`) wired into `AppView`.
- Four dock regions established representing the After Effects-style workspace:
  - Left dock (`DockPlacement::Left`, 280px): `ProjectPanel` (Asset bin / project media list placeholder)
  - Center dock (`DockPlacement::Center`): `CompositionViewerPanel` (Composition viewport canvas, resolution & aspect ratio placeholder)
  - Right dock (`DockPlacement::Right`, 300px): Tab group containing `PropertiesPanel` and `EffectsPanel`
  - Bottom dock (`DockPlacement::Bottom`, 260px): `TimelinePanel` (Timecode display, transport controls, time ruler, and layer tracks)
- Five domain-appropriate placeholder panels implemented in `crates/application/src/panels.rs`:
  1. `ProjectPanel`: Search bar, action buttons, table column headers, mock items (Composition, Video, Image, Audio), and item count status footer (`project_assets`).
  2. `CompositionViewerPanel`: Viewport top bar with camera/resolution/color channel controls, 16:9 aspect-ratio composition canvas frame, timecode readout, and pan/zoom status footer (`composition_viewer`).
  3. `PropertiesPanel`: Selected layer header, full transform inspector fields (Position, Scale, Rotation, Opacity, Anchor Point), and layer switches/modes (`properties_inspector`).
  4. `EffectsPanel`: Search bar and categorized built-in effects list (`effects_categories`) across 5 categories (Blur & Sharpen, Color Correction, Distort, Generate, Transition; 18 built-in effects).
  5. `TimelinePanel`: Digital timecode readout, transport controls (Play/Pause, Loop, Skip), work area markers, time ruler with second intervals, and multi-layer track lane representations (`timeline`).
- Each panel implements `BasePanel`, `Panel`, `Focusable`, `EventEmitter<PanelEvent>`, and `Render` with `.id(...)`, `.test_support()`, and `.track_focus(&self.focus_handle)`.
- Core project data model crate created under `crates/project`, completely decoupled from UI:
  - `Project`: Versioned root container (`format_version`, `id`, `name`, `compositions`, `assets`, `settings`).
  - `Composition`: (`id`, `name`, `width`, `height`, `frame_rate`, `duration`, `background_color`, `layers`, `markers`).
  - `Layer`: (`id`, `name`, `source`, `transform`, `opacity`, `blend_mode`, `visible`, `locked`, `in_point`, `out_point`, `parent_id`, `markers`).
  - `LayerSource`: Rich enum supporting `Solid`, `Image`, `Video`, `Text`, `Shape` (Rectangle, Ellipse, Path), `NestedComposition`, and `Procedural`.
  - `Property<T>`: Uniform animatable property system (`name`, `value`, `default_value`, `animated`, `reset`, `is_default`, `Deref`/`DerefMut`).
  - `Transform`: Spatial transform with `anchor_point: Property<Vec2>`, `position: Property<Vec2>`, `scale: Property<Vec2>`, and `rotation: Property<f32>`.
  - `Asset`: Media asset container (`id`, `name`, `path`, `asset_type`) with automatic extension detection and cross-composition referencing.
  - Supporting types: `BlendMode` (19 blend modes), `Color` (RGBA float, hex parsing/formatting, clamping), `TimeCode` (frames, seconds, frame rate, drop-frame, arithmetic, formatting, negative timecode support), and `Marker`.
  - Full structural and hierarchical validation: dimension/timing bounds, layer ordering, parent cycle detection, asset tracking, deep parenting chains (tested to depth 100), and recursive nested composition cycle detection (tested to depth 60).
- Full project serialization and file persistence implemented in `crates/project`:
  - Conforms to Section 16 format (`format_version`, `project: { id, name }`, `compositions`, `assets`, `settings`).
  - Supports dual format deserialization (nested Section 16 metadata or flat identifier format).
  - All domain types support `serde::Serialize` and `serde::Deserialize` (`Project`, `ProjectSettings`, `Composition`, `Layer`, `LayerSource`, `ShapeType`, `Transform`, `Property<T>`, `Vec2`, `Color`, `TimeCode`, `Marker`, `Asset`, `AssetType`, `BlendMode`).
  - `BlendMode` serializes to canonical snake_case strings and deserializes case-insensitively with space and hyphen support.
  - Polymorphic layer types serialize with `{"type": "..."}` discriminant tagging.
  - Project file operations provided: `to_json()`, `to_json_pretty()`, `from_json()`, `save_to_file()`, and `load_from_file()`.
- Pure Rust composition evaluation engine initiated under `crates/compositor`:
  - `SceneNode`: Maps composition layer data with stack order index, transform, source, timing, blend mode, solo, matte mode, and hierarchy connections (`parent_id`, `children_ids`).
  - `SceneGraph`: Hierarchical scene graph representation with dual ordering systems:
    - **Topological Evaluation Order**: Resolves parent-child spatial dependencies so that parents are guaranteed to evaluate before children.
    - **Composite Order (Painter's Algorithm)**: Bottom-to-top rendering order where lower layers are rasterized first and upper layers composite over them.
    - **Timeline Layer Stacking**: Original layer stack order preserving timeline UI visual arrangement.
  - `TrackMatteMode`: Full track matte modeling (`None`, `Alpha`, `AlphaInverted`, `Luma`, `LumaInverted`) with serialization and predicate helpers.
  - `LayerStackEvaluator`: Evaluates composition layers at any timeline `TimeCode`, enforcing exact `[in_point, out_point)` boundary intervals, solo suppression/preservation, track matte pairing (explicit and adjacent), opacity clamping/normalization, and painter's composite render list generation.
  - `EvaluatedStack` & `EvaluatedLayer`: Comprehensive frame evaluation context with timing offsets, layer states, and filter queries.
  - `AffineTransform2D`: Full 2D affine transformation matrix math supporting translations, non-uniform scaling, clockwise rotations (degrees and radians), anchor point offset, matrix multiplication, inversion, and $3\times3$ conversions.
  - `BoundingBox2D`: 2D axis-aligned bounding box primitive with affine transform projection, union, intersection, point containment, and corner evaluation.
  - `TransformResolver`: Evaluates layer world transforms via hierarchical matrix concatenation ($M_{\text{world}} = M_{\text{parent}} \times M_{\text{local}}$) in topological evaluation order at arbitrary timeline positions (`resolve_scene_graph_at`).
  - `EvaluatedTransform`: Evaluated local and world matrices with bidirectional point and bounding box mapping (`local_to_world_point`, `world_to_local_point`, `local_to_world_bbox`, `world_to_local_bbox`, and `world_bounds`), with time-based construction (`from_node_transform_at`).
- Complete animatable property system and interpolation engine implemented across `crates/project` and `crates/compositor`:
  - `Interpolate`: Pure Rust interpolation trait supporting linear interpolation (`lerp`) and step/hold interpolation (`step`) across scalar floats (`f32`, `f64`), 2D vectors (`Vec2`), colors (`Color`), booleans, and strings.
  - `Keyframe<T>`: Rich keyframe container holding `time: TimeCode`, `subframe: f32`, `value: T`, `interpolation: KeyframeInterpolation`, and optional `in_tangent` / `out_tangent` handles (`KeyframeTangent`).
  - `KeyframeInterpolation`: `Hold` (step), `Linear`, and `Bezier` easing modes.
  - `KeyframeTangent`: 2D normalized control handle $(x, y)$ with industry presets (`linear_out`, `linear_in`, `ease_in`, `ease_out`, `ease_in_out`).
  - `Extrapolation`: Pre- and post-boundary extrapolation modes (`Hold`, `Linear` tangent slope projection, `Cycle` periodic loop, `PingPong` reflection).
  - Cubic Bezier Solver: Analytical Newton-Raphson inversion of $x(\theta) = t$ with robust bisection fallback and $y(\theta)$ evaluation.
  - Keyframe Track Evaluator: Binary search lookup across sorted keyframe tracks with exact point matching, interval interpolation, and sub-frame floating-point evaluation.
  - `Property<T>`: Extended with `keyframes: Vec<Keyframe<T>>`, sorted insertion (`add_keyframe`), timestamp replacement, removal, and continuous evaluation (`evaluate_at`, `evaluate_at_seconds`, `evaluate_with_extrapolation`).
  - Full backward compatibility: `keyframes` serialized with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
  - Compositor integration: `LayerStackEvaluator` evaluates animated layer opacity and animated hierarchical transforms dynamically at the target `TimeCode`.
- High-precision playback clock and timeline transport architecture implemented in `crates/project`:
  - `FrameRate`: Precise rational frame rate representation (`numerator`, `denominator`, `drop_frame`) preventing accumulated floating-point inaccuracies, with presets for all industry standards (23.976, 24, 25 PAL, 29.97 NDF, 29.97 DF, 30, 50, 59.94 NDF, 59.94 DF, 60, and arbitrary rational custom rates).
  - SMPTE 12M drop-frame standard calculation: exact frame-to-timecode and timecode-to-frame algorithms dropping frames 0 and 1 at each minute except every 10th minute for 29.97 DF (and frames 0, 1, 2, 3 for 59.94 DF), with `;` separator support and dropped frame detection (`TimeCodeError::DroppedFrame`).
  - `PlaybackClock` / `Transport`:
    - Drift-free integer nanosecond continuous position tracking (`position_nanos`, `position_seconds`) and discrete frame quantization (`current_frame`, `subframe` in `[0.0, 1.0)`).
    - Transport states: `Playing`, `Paused`, `Scrubbing` with full transition methods (`play`, `pause`, `toggle_playback`, `play_reverse`, `start_scrubbing`, `stop_scrubbing`, `scrub_to`, `seek`).
    - Speed multiplier and reverse playback support (positive/negative speeds, `set_speed`, `reverse`).
    - Work area looping and boundaries: `WorkArea` (`in_point`, `out_point`) with loop modes (`Loop` wrap-around, `Once` boundary stop, `PingPong` velocity bounce).
    - Drift-free delta time advancement: `tick(dt: std::time::Duration)` returning `ClockTickResult` (`timecode`, `frame`, `frame_changed`, `looped`, `reached_end`).
    - Navigation and stepping: single/multi-frame stepping (`step_next_frame`, `step_prev_frame`, `step_forward`, `step_backward`), jump to work area bounds (`jump_to_start`, `jump_to_end`), jump to composition bounds, and keyframe/marker jumping (`jump_to_next_keyframe_in_comp`, `jump_to_prev_keyframe_in_comp`, `jump_to_next_marker_in_comp`, `jump_to_prev_marker_in_comp`).
- Full automated test suite passing with **92 tests** across the workspace (12 application tests + 49 project unit tests + 31 compositor unit tests).
- Workspace compiles, builds, and passes all checks cleanly with 0 errors and 0 warnings (`cargo check --workspace`, `cargo build --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`).

## What Is Currently Being Developed
- Phase 1: Core Engine Architecture.

## Known Problems
- None.

## Next Single Task

### Task 1.6: Implement composition nesting and pre-comp evaluation

- Design nested composition evaluation pipeline in `crates/compositor`.
- Implement pre-comp time-remapping, frame offset, and duration clipping relative to parent composition timelines.
- Connect nested composition scene nodes to upstream evaluated stacks without circular graph recursion.
- Add automated unit tests covering multi-level nested compositions, transform propagation across nesting boundaries, and timing offsets.

