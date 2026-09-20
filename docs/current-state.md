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
- Composition nesting and pre-comp evaluation pipeline implemented across `crates/project` and `crates/compositor`:
  - `NestedCompositionEvaluation`: encapsulates evaluated inner stack, resolved local timeline timecode, composition canvas dimensions, and concatenated nesting world transform matrix.
  - Time Remapping & Temporal Alignment:
    - Base time offset calculation: $t_{\text{nested}} = t_{\text{parent}} - t_{\text{in\_point}} + t_{\text{start\_offset}}$.
    - Time stretch / speed factor support: positive/negative speed multipliers (e.g. 50% slow-motion, 200% double speed, negative reverse).
    - Optional animated time remapping property (`time_remapping: Option<Property<f64>>`) mapping parent timeline to inner composition seconds.
    - Duration bounds clamping and loop modes: `LoopMode::Once` (boundary clamp), `LoopMode::Loop` (periodic wrap), and `LoopMode::PingPong` (oscillation bounce).
    - Multi-rate synchronization: frame-exact calculations for matched frame rates and floating-point continuous second mapping for disparate frame rates, with non-finite float guards.
  - Spatial Concatenation & Nested Transformations:
    - Hierarchical world transform cascading: $M_{\text{inner\_root}} = M_{\text{nesting\_world\_matrix}} \times M_{\text{inner\_world}}$.
    - Bidirectional point transformations: `inner_to_root_point` and `root_to_inner_point`.
    - Bounding box mapping: `inner_to_root_bbox`, `canvas_bounds_in_root`, and exact `inner_layer_root_bounds` (direct root transformation eliminating intermediate AABB rotation bloat).
    - Transform helper `transform_bbox` on `AffineTransform2D`.
  - Recursive Frame Representation & Compositing:
    - `EvaluatedLayer` embeds `nested_composition: Option<Box<NestedCompositionEvaluation>>` with query methods `is_nested_composition()` and `nested_evaluation()`.
    - `FlattenedRenderLayer`: represents atomic render elements with fully resolved root world transforms, cumulative opacities, hierarchical layer paths, `matte_source_path`, `is_matte_source`, and nesting depths.
    - `EvaluatedStack`: provides `flattened_render_list()` expanding nested compositions in painter's composite order, `collect_render_passes()` for offscreen render targets in dependency order (including track matte pre-comps with `nesting_layer_id`), `has_nested_compositions()`, `get_layer_deep()`, `get_flattened_layer()`, and `deep_layer_root_matrix()`.
  - Recursion Limits & Cycle Protection:
    - `LayerStackEvaluator`: configurable `max_nesting_depth` (default 32) guarding against unbounded recursion with `SceneGraphError::MaxNestingDepthExceeded`.
    - Active cycle detection detecting circular nested composition references with `SceneGraphError::CircularNestedComposition`.
    - Graceful missing composition error handling with `SceneGraphError::CompositionNotFound`.
  - Layer Model Integration in `crates/project`:
    - Extended `Layer` with `start_offset`, `time_stretch`, `time_remapping`, and `loop_mode` with full backward compatibility and JSON serialization roundtripping.
- Comprehensive headless integration test suite and performance benchmark implemented in `crates/compositor/tests/headless_evaluation_tests.rs`:
  - 8 realistic end-to-end composition scenario tests exercising all engine features headless:
    - Multi-layer composite with mixed layer sources (Solids, Text, Shape - Rectangle, Ellipse, Path, Procedural, NestedComposition) with activation boundaries and flattened render lists.
    - Deep 6-level parenting chains with Bezier ease-in-out curves across Position, Scale, Rotation, and Anchor Point, verifying topological evaluation order, parent-child matrix concatenation, and numerical invertibility.
    - All 4 track matte modes (`Alpha`, `AlphaInverted`, `Luma`, `LumaInverted`) with adjacent and explicit targeting, matte consumption, and matte preservation under soloing.
    - 3-tier nested pre-comps with time stretching (50% slow-mo, 200% double speed, reverse playback), start offsets, animated time remapping, and loop modes (`Once`, `Loop`, `PingPong`).
    - Full 300-frame timeline scrub verifying half-open interval boundaries `[in_point, out_point)`, opacity fades, zero-opacity render omission, and painter's composite order.
    - Full tree point mapping: bidirectional point mapping from innermost nested pre-comp layer to root composition viewport coordinates across cascaded nesting transforms, verifying exact invertibility and root bounding box containment.
    - Defensive validation of parenting cycles, self-parenting, missing parents, circular nested compositions, missing composition references, recursion depth limits (`MaxNestingDepthExceeded`), negative timecodes, and extreme out-of-bounds frame requests.
    - High-throughput stress test evaluating 10,000 frames of a complex multi-layer composition (12 layers, parenting chains, Bezier curves, track mattes, nested pre-comps) achieving ~16,000 evaluations/sec in debug mode and > 72,000 evaluations/sec in release mode (~13.9 µs/frame latency), vastly exceeding real-time timeline scrubbing requirements.
- Full engine-to-UI integration completed in `crates/application`:
  - `EditorState` in `crates/application/src/state.rs`: Manages active `Project`, `PlaybackClock`, active composition selection, layer selection, demo seed composition, layer mutations (transform nudges, visibility, solo, layer creation), and real-time frame evaluation via `evaluate_current_frame()`.
  - Reactive GPUI panels in `crates/application/src/panels.rs`:
    - `ProjectPanel`: Displays live project composition and layer assets, interactive row selection, and working `+ Solid` button adding new solid layers dynamically to the composition.
    - `CompositionViewerPanel`: Observes `EditorState`, evaluates frames at current playback timecode, maps world bounding boxes to canvas dimensions (512x288 16:9 frame), renders visible layers in painter's composite order with effective opacities, supports click selection, and highlights selected layer with an accent border.
    - `PropertiesPanel`: Real-time inspector for the selected layer displaying dynamic layer type (Solid, Image, Video, Text, Shape, Pre-comp), Anchor, Position, Scale, Rotation, and true Opacity percentage (evaluated at active timecode) with clickable `-`/`+` nudge step buttons, and toggleable `[✓] Visible` and `[✓] Solo` switches (empty state when no layer is selected).
    - `TimelinePanel`: Live SMPTE timecode and frame counter, transport buttons (`|<`, `<`, `▶ Play`/`⏸ Pause`, `>`, `>|`), time ruler with dynamic playhead marker, and layer track lanes with index, `[V]` eye toggle, `[S]` solo toggle, interactive clickable track span bars for layer selection, selection state, and track playhead line.
  - Application shell & input in `crates/application/src/main.rs`:
    - Spacebar keybinding and `TogglePlayback` action for play/pause control.
    - 60Hz asynchronous background playback loop advancing `PlaybackClock` with delta time and notifying GPUI observers.
- Hardware-Accelerated Rendering Architecture initiated in `crates/renderer` (Phase 2 Task 2.1):
  - `GpuContext`: Headless wgpu 24 initialization with automatic hardware adapter selection and software adapter fallback, downlevel capabilities configuration, and device/queue accessors.
  - `RenderTarget`: Standard `Rgba8Unorm` texture allocation (`RENDER_ATTACHMENT | COPY_SRC | TEXTURE_BINDING`), color `TextureView`, and CPU texture readback (`read_texture_to_cpu`) handling wgpu 256-byte alignment (`COPY_BYTES_PER_ROW_ALIGNMENT`).
- Real Lucide Vector Icons Integration across GPUI panels (`gpui_kit::assets::IconName`):
  - Timeline transport: `SkipBack`, `StepBack`, `Play`/`Pause`, `StepForward`, `SkipForward`, `Repeat`.
  - Layer switches: `Eye`/`EyeOff` for visibility, `Sparkles` for solo.
  - Project panel: `Folder`, `Layers`, `Image`, `Film`, `Music`, `Type`, `Sparkles`, `FolderOpen` ("Import Media..."), `Plus` ("Solid").
  - Properties inspector: `Layers`, `Move`, `Maximize2`, `RotateCw`, `Sun`, `SlidersHorizontal`, `Eye`/`EyeOff`, `Trash`.
  - Effects panel: `SlidersHorizontal`, `Palette`, `WandSparkles`, `Sparkles`, `RotateCw`, `Plus`.
- Interactive Horizontal Scrubbing Properties (`PropertiesPanel`):
  - After Effects / Blender style horizontal mouse drag scrubbing (`cursor_col_resize`) on value fields to smoothly increase/decrease values.
  - Scroll wheel support with directional step increments.
  - `-`/`+` step buttons with test IDs for Anchor Point, Position, Scale, Rotation, Opacity, and all effect parameters.
- Comprehensive Layer Reordering & Deletion:
  - Global `Delete` and `Backspace` keyboard shortcuts delete the currently selected layer immediately.
  - Timeline track rows feature direct Move Up (`ChevronUp`), Move Down (`ChevronDown`), and Delete (`Trash`) buttons.
  - Properties panel header and Timeline header provide Move Up, Move Down, and Delete action buttons for the active layer.
  - `EditorState::move_selected_layer_up`, `move_selected_layer_down`, `delete_selected_layer`, `remove_layer_by_id`.
- Real Visible Canvas Effects & Custom GLSL Shaders:
  - Gaussian blur visual aura box rendered dynamically behind blurred layers.
  - Canvas overlays for Tint and Invert effects on image layers.
  - Video preview cards with filmstrip badges, duration, and resolution indicators.
  - Custom GLSL Shader effect with interactive parameter controls (`param1`..`param4`), code preview, and preset bar (`Default Boost`, `Color Wave`, `Glow Shimmer`, `CRT Scanlines`).
- Media Import & Instant Testing Generators:
  - `EditorState::import_sample_image`: Generates a 400x400 PNG gradient file to the temp directory and imports it into the project and active composition for instant testing.
  - `EditorState::import_sample_video`: Generates a demo video file placeholder and imports it.
- Project Panel UI Decluttering:
  - Clutter removed: replaced repetitive duplicate full layer list with clean categorized bins (`Compositions`, `Imported Media Assets`, `Solids & Generators`).
- Full automated test suite passing with **139 tests** across the workspace (30 application tests + 51 project unit tests + 47 compositor unit tests + 8 compositor integration tests + 3 renderer unit tests).
- Workspace compiles, builds, and passes all checks cleanly with 0 errors and 0 warnings (`cargo check --workspace`, `cargo build --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`).

## What Is Currently Being Developed
- Phase 2: Hardware-Accelerated Rendering Architecture (pipelines, shaders, layer blit).

## Known Problems
- None.

## Next Single Task

### Task 2.2: Create offscreen render target and readback

- Expand render target abstraction with multi-buffer pipelining and color format conversions.
- Create vertex/fragment shader pipelines for compositing layers onto render targets.
- Implement texture cache for uploaded assets and cached pre-comps.


