# Architecture

## Overview

The motion graphics and compositing application follows a modular architecture separating the native editor UI from the underlying composition engine, project data model, and hardware-accelerated render pipeline.

## System Layers

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

## Workspace Crates

- `crates/application`: The native executable entry point, application bootstrap, and lifecycle management.
- `crates/ui` *(planned Phase 1)*: Editor UI components, panel implementations, docking layout, and styling.
- `crates/project`: Pure Rust data structures for Project, Composition, Layer, Asset, Property system, and validation.
- `crates/compositor`: Composition evaluation engine, scene graph representation, topological evaluation sort, and composite ordering.
- Future crates (`renderer`, `timeline`, `animation`, `media`, `effects`, `cache`, etc.) will be introduced incrementally as their phases begin.
