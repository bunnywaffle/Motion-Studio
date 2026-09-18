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
  - `SceneNode`: Maps composition layer data with stack order index, transform, source, timing, blend mode, and hierarchy connections (`parent_id`, `children_ids`).
  - `SceneGraph`: Hierarchical scene graph representation with dual ordering systems:
    - **Topological Evaluation Order**: Resolves parent-child spatial dependencies so that parents are guaranteed to evaluate before children.
    - **Composite Order (Painter's Algorithm)**: Bottom-to-top rendering order where lower layers are rasterized first and upper layers composite over them.
    - **Timeline Layer Stacking**: Original layer stack order preserving timeline UI visual arrangement.
  - Hierarchy query methods: `get_parent`, `get_children`, `get_ancestor_chain`, `get_descendants` (BFS), `depth_of`, and `root_nodes`.
  - Timecode activity and visibility filtering (`active_nodes_at`, `render_order_at`).
  - Cycle detection, self-parenting prevention, and missing parent validation with `SceneGraphError`.
- Full automated test suite passing with **49 tests** across the workspace (12 application tests + 28 project unit tests + 9 compositor unit tests).
- Workspace compiles, builds, and passes all checks cleanly with 0 errors and 0 warnings (`cargo check --workspace`, `cargo build --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`).

## What Is Currently Being Developed
- Phase 1: Core Engine Architecture.

## Known Problems
- None.

## Next Single Task

### Task 1.2: Implement the layer stack evaluation model

- Implement the layer evaluation pipeline that processes layers in the composition according to the scene graph.
- Support evaluating layer states at specific timeline positions, taking into account in/out points, track matte, and opacity.
- Add unit tests verifying layer evaluation at varied timecodes, boundary conditions, and disabled/hidden layer handling.
