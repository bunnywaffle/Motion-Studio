# Motion Studio

A native, cross-platform After Effects-style motion graphics and 2D compositing desktop application built in Rust with GPUI Kit and wgpu.

## Overview

Motion Studio is designed for:
- Motion graphics design and vector animation
- 2D compositing and multi-layer blending
- Video and audio sequencing
- Procedural visual effects and procedural graphics
- Extensible effect pipelines

## Architecture

Motion Studio employs a clean, decoupled modular workspace architecture:

```text
+-------------------------------------------------------------+
|                          GPUI Kit                           |
|      (Main Window, Native Menus, Docking, Panels, Tabs)      |
+-------------------------------------------------------------+
                              |
+-------------------------------------------------------------+
|                          Editor UI                          |
|  (Project Panel, Viewer Panel, Properties, Timeline, etc.)  |
+-------------------------------------------------------------+
                              |
+-------------------------------------------------------------+
|                       Application API                       |
|           (Commands, Undo/Redo, Project Management)         |
+-------------------------------------------------------------+
                              |
+-------------------------------------------------------------+
|                         Compositor                          |
|             (Evaluation, Render Graph, Caching)             |
+-------------------------------------------------------------+
                              |
+-------------------------------------------------------------+
|                          Renderer                           |
|                       (wgpu Pipeline)                       |
+-------------------------------------------------------------+
```

### Crates
- `crates/application`: Native desktop binary, GPUI Kit application bootstrap, dark theme integration, and 4-region docking layout (`ProjectPanel`, `CompositionViewerPanel`, `PropertiesPanel`, `TimelinePanel`, `EffectsPanel`).
- `crates/project`: Pure Rust core project model (`Project`, `Composition`, `Layer`, `LayerSource`, `Transform`, `Property<T>`, `Asset`, `BlendMode`, `Color`, `TimeCode`, `Marker`) with full serialization conforming to Section 16 format.
- `crates/compositor`: Pure Rust scene graph and composition evaluation engine supporting topological evaluation sort (resolving parent-child transform dependencies) and painter's composite rendering order.

## Building and Running

### Prerequisites
- [Rust](https://www.rust-lang.org/) (edition 2021, 1.80+)
- Windows, macOS, or Linux

### Build & Run
```bash
# Build the workspace
cargo build --workspace

# Run the desktop application
cargo run -p application

# Run the test suite
cargo test --workspace

# Run Clippy checks
cargo clippy --workspace --all-targets -- -D warnings
```

## Status
- **Phase 0 (Repository & Build System)**: 100% Complete
- **Phase 1 (Core Engine Architecture)**: In Progress (Task 1.1 Complete)

## License
MIT OR Apache-2.0
