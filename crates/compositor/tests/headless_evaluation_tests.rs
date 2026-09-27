use compositor::{LayerStackEvaluator, SceneGraph, SceneGraphError};
use project::{
    Color, Composition, Keyframe, KeyframeTangent, LoopMode, Project, Property, ShapeType,
    TimeCode, TrackMatteMode, Vec2,
};
use std::time::Instant;

// 1. Multi-layer composite with mixed layer sources (Solids, Text, Shape, NestedCompositions)
#[test]
fn test_multi_layer_mixed_sources_composite_evaluation() {
    let mut project = Project::with_defaults("Mixed Sources Project");
    let fps = 60.0;
    let tc0 = TimeCode::from_frames(0, fps);
    let tc300 = TimeCode::from_frames(300, fps); // 5 seconds at 60fps

    // Child Comp: Lower Third Graphic (1920x200, 5 seconds)
    let mut lower_third = Composition::new("comp_lower_third", "Lower Third", 1920, 200, fps, tc300);
    let lt_bg = project::Layer::solid("lt_bg", "LT Background", Color::rgba(0.1, 0.1, 0.2, 0.8), 1920, 200, tc0, tc300);
    let lt_text = project::Layer::text(
        "lt_name",
        "Speaker Name",
        "Jane Doe - Lead Animator",
        "Inter",
        48.0,
        Color::WHITE,
        tc0,
        tc300,
    );
    lower_third.add_layer(lt_bg).unwrap();
    lower_third.add_layer(lt_text).unwrap();

    // Root Comp: 1920x1080 60fps
    let mut root_comp = Composition::new("comp_root", "Root Composition", 1920, 1080, fps, tc300);

    // Layer 0: Background Solid
    let bg_solid = project::Layer::solid("bg_solid", "Background", Color::BLACK, 1920, 1080, tc0, tc300);

    // Layer 1: Vector Shape - Rectangle
    let rect_shape = project::Layer::shape(
        "rect_accent",
        "Accent Rectangle",
        ShapeType::Rectangle {
            width: Property::new("Width", 500.0),
            height: Property::new("Height", 10.0),
            corner_radius: Property::new("Corner Radius", 5.0),
            fill: Color::WHITE,
        fill_gradient: None,
        },
        tc0,
        tc300,
    );

    // Layer 2: Vector Shape - Ellipse
    let ellipse_shape = project::Layer::shape(
        "ellipse_glow",
        "Glow Circle",
        ShapeType::Ellipse {
            radius_x: Property::new("Radius X", 80.0),
            radius_y: Property::new("Radius Y", 80.0),
            fill: Color::WHITE,
        fill_gradient: None,
        },
        tc0,
        tc300,
    );

    // Layer 3: Vector Shape - Path
    let path_shape = project::Layer::shape(
        "path_triangle",
        "Arrow Head",
        ShapeType::Path {
            path_data: "M 0 0 L 40 20 L 0 40 Z".to_string(),
            fill: Color::WHITE,
        fill_gradient: None,
        },
        tc0,
        tc300,
    );

    // Layer 4: Typography Text Layer
    let main_title = project::Layer::text(
        "title_text",
        "Main Title",
        "Motion Graphics Studio",
        "Inter",
        72.0,
        Color::WHITE,
        tc0,
        tc300,
    );

    // Layer 5: Nested Composition (Lower Third) positioned at bottom
    let mut lt_nest = project::Layer::nested_composition(
        "nest_lower_third",
        "Lower Third Overlay",
        "comp_lower_third",
        TimeCode::from_frames(60, fps), // starts at 1.0s
        TimeCode::from_frames(240, fps), // ends at 4.0s
    );
    lt_nest.transform.position.set_value(Vec2::new(0.0, 880.0));

    // Layer 6: Procedural layer
    let procedural_layer = project::Layer::new(
        "proc_noise",
        "Noise Overlay",
        project::LayerSource::Procedural {
            generator_type: "perlin_noise".to_string(),
        },
        tc0,
        tc300,
    );

    root_comp.add_layer(bg_solid).unwrap();
    root_comp.add_layer(rect_shape).unwrap();
    root_comp.add_layer(ellipse_shape).unwrap();
    root_comp.add_layer(path_shape).unwrap();
    root_comp.add_layer(main_title).unwrap();
    root_comp.add_layer(lt_nest).unwrap();
    root_comp.add_layer(procedural_layer).unwrap();

    project.add_composition(root_comp).unwrap();
    project.add_composition(lower_third).unwrap();

    let evaluator = LayerStackEvaluator::new();

    // Evaluate at Frame 30 (0.5s): Lower third pre-comp is inactive (< frame 60)
    let stack_30 = evaluator
        .evaluate_composition(&project, "comp_root", &TimeCode::from_frames(30, fps))
        .expect("Evaluate frame 30");

    assert_eq!(stack_30.evaluated_layers.len(), 7);
    assert_eq!(stack_30.active_count(), 6); // all except nest_lower_third
    let lt_eval_30 = stack_30.get_layer("nest_lower_third").unwrap();
    assert!(!lt_eval_30.is_active);
    assert!(lt_eval_30.nested_composition.is_none());

    // Check layer source types
    assert_eq!(stack_30.get_layer("bg_solid").unwrap().source.type_name(), "Solid");
    assert_eq!(stack_30.get_layer("rect_accent").unwrap().source.type_name(), "Shape");
    assert_eq!(stack_30.get_layer("ellipse_glow").unwrap().source.type_name(), "Shape");
    assert_eq!(stack_30.get_layer("path_triangle").unwrap().source.type_name(), "Shape");
    assert_eq!(stack_30.get_layer("title_text").unwrap().source.type_name(), "Text");
    assert_eq!(stack_30.get_layer("nest_lower_third").unwrap().source.type_name(), "NestedComposition");
    assert_eq!(stack_30.get_layer("proc_noise").unwrap().source.type_name(), "Procedural");

    // Evaluate at Frame 120 (2.0s): Lower third pre-comp is active
    let stack_120 = evaluator
        .evaluate_composition(&project, "comp_root", &TimeCode::from_frames(120, fps))
        .expect("Evaluate frame 120");

    assert_eq!(stack_120.active_count(), 7);
    let lt_eval_120 = stack_120.get_layer("nest_lower_third").unwrap();
    assert!(lt_eval_120.is_active);
    assert!(lt_eval_120.is_visible);
    let nested_inner = lt_eval_120.nested_evaluation().expect("Has nested evaluation");
    assert_eq!(nested_inner.composition_id, "comp_lower_third");
    assert_eq!(nested_inner.width, 1920);
    assert_eq!(nested_inner.height, 200);
    assert_eq!(nested_inner.resolved_time.frames(), 60); // 120 - 60 = 60 frames

    // Painter's composite order in root: bottom to top
    // Stack order: [0: bg, 1: rect, 2: ellipse, 3: path, 4: title, 5: nest, 6: proc]
    // Painter's composite order: proc (6) -> nest (5) -> title (4) -> path (3) -> ellipse (2) -> rect (1) -> bg (0)
    assert_eq!(
        stack_120.render_list,
        vec![
            "proc_noise",
            "nest_lower_third",
            "title_text",
            "path_triangle",
            "ellipse_glow",
            "rect_accent",
            "bg_solid"
        ]
    );

    // Flattened render list expands nest_lower_third in composite order
    let flattened = stack_120.flattened_render_list();
    let flat_ids: Vec<&str> = flattened.iter().map(|l| l.layer_id.as_str()).collect();
    // In lower_third: [0: lt_bg, 1: lt_text]. Composite order: lt_text -> lt_bg
    assert_eq!(
        flat_ids,
        vec![
            "proc_noise",
            "lt_name",
            "lt_bg",
            "title_text",
            "path_triangle",
            "ellipse_glow",
            "rect_accent",
            "bg_solid"
        ]
    );

    // Inner layer root world matrix verifies offset (0, 880)
    let lt_name_flat = flattened.iter().find(|l| l.layer_id == "lt_name").unwrap();
    assert_eq!(lt_name_flat.nesting_depth, 1);
    assert_eq!(lt_name_flat.layer_path, vec!["nest_lower_third", "lt_name"]);
    assert_eq!(lt_name_flat.root_world_matrix.ty, 880.0);
}

// 2. Deep parenting chains combined with keyframed transform animations (Position, Scale, Rotation, Anchor Point)
// using Bezier ease-in-out curves.
#[test]
fn test_deep_parenting_hierarchy_with_bezier_transform_curves() {
    let fps = 30.0;
    let mut comp = Composition::hd_1080p_30fps("comp_deep_bezier", "Deep Bezier Parenting", 5.0);
    let tc0 = TimeCode::from_frames(0, fps);
    let tc30 = TimeCode::from_frames(30, fps);
    let tc60 = TimeCode::from_frames(60, fps);

    // Chain: root (l0) -> l1 -> l2 -> l3 -> l4 -> l5
    const DEPTH: usize = 6;
    for i in 0..DEPTH {
        let mut layer = project::Layer::solid(
            format!("node_{i}"),
            format!("Node {i}"),
            Color::WHITE,
            100,
            100,
            tc0,
            tc60,
        );

        if i > 0 {
            layer.set_parent(Some(format!("node_{}", i - 1)));
        }

        // Apply distinct Bezier animated properties across different levels
        match i {
            0 => {
                // Root node: Position animated (100, 100) -> (500, 300) with Bezier ease-in-out
                layer.transform.position.add_keyframe(Keyframe::bezier(
                    tc0,
                    Vec2::new(100.0, 100.0),
                    None,
                    Some(KeyframeTangent::ease_in_out_out()),
                ));
                layer.transform.position.add_keyframe(Keyframe::bezier(
                    tc30,
                    Vec2::new(500.0, 300.0),
                    Some(KeyframeTangent::ease_in_out_in()),
                    None,
                ));
            }
            1 => {
                // Node 1: Rotation animated 0 -> 90 degrees with Bezier ease-in-out
                layer.transform.position.set_value(Vec2::new(50.0, 0.0));
                layer.transform.rotation.add_keyframe(Keyframe::bezier(
                    tc0,
                    0.0,
                    None,
                    Some(KeyframeTangent::ease_in_out_out()),
                ));
                layer.transform.rotation.add_keyframe(Keyframe::bezier(
                    tc30,
                    90.0,
                    Some(KeyframeTangent::ease_in_out_in()),
                    None,
                ));
            }
            2 => {
                // Node 2: Scale animated 100% -> 200% with Bezier ease-in-out
                layer.transform.position.set_value(Vec2::new(0.0, 40.0));
                layer.transform.scale.add_keyframe(Keyframe::bezier(
                    tc0,
                    Vec2::SCALE_100,
                    None,
                    Some(KeyframeTangent::ease_in_out_out()),
                ));
                layer.transform.scale.add_keyframe(Keyframe::bezier(
                    tc30,
                    Vec2::new(200.0, 200.0),
                    Some(KeyframeTangent::ease_in_out_in()),
                    None,
                ));
            }
            3 => {
                // Node 3: Anchor Point animated (0, 0) -> (50, 50)
                layer.transform.position.set_value(Vec2::new(30.0, 30.0));
                layer.transform.anchor_point.add_keyframe(Keyframe::bezier(
                    tc0,
                    Vec2::ZERO,
                    None,
                    Some(KeyframeTangent::ease_in_out_out()),
                ));
                layer.transform.anchor_point.add_keyframe(Keyframe::bezier(
                    tc30,
                    Vec2::new(50.0, 50.0),
                    Some(KeyframeTangent::ease_in_out_in()),
                    None,
                ));
            }
            4 => {
                // Node 4: Position animated along Y (0, 0) -> (0, 100)
                layer.transform.position.add_keyframe(Keyframe::linear(tc0, Vec2::ZERO));
                layer.transform.position.add_keyframe(Keyframe::linear(tc30, Vec2::new(0.0, 100.0)));
            }
            5 => {
                // Leaf Node 5: Static offset
                layer.transform.position.set_value(Vec2::new(10.0, 20.0));
            }
            _ => {}
        }

        comp.add_layer(layer).unwrap();
    }

    let graph = SceneGraph::from_composition(&comp).expect("Valid graph");
    let evaluator = LayerStackEvaluator::new();

    // Verify topological order
    let topo = graph.evaluation_order().unwrap();
    let topo_ids: Vec<&str> = topo.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(topo_ids, vec!["node_0", "node_1", "node_2", "node_3", "node_4", "node_5"]);

    // Test evaluation at Frame 0, 15 (midpoint), 30 (end), 45
    for frame_idx in [0, 15, 30, 45] {
        let t = TimeCode::from_frames(frame_idx, fps);
        let stack = evaluator.evaluate(&graph, &t);

        // Verify that every child's world matrix equals Parent World Matrix * Child Local Matrix
        for i in 1..DEPTH {
            let parent_layer = stack.get_layer(&format!("node_{}", i - 1)).unwrap();
            let child_layer = stack.get_layer(&format!("node_{i}")).unwrap();

            let expected_world = parent_layer.world_matrix() * child_layer.local_matrix();
            assert!(
                child_layer.world_matrix().approx_eq(&expected_world, 1e-3),
                "Matrix mismatch at depth {i} frame {frame_idx}"
            );
        }

        // Test invertibility and roundtrip point mapping for leaf node
        let leaf = stack.get_layer("node_5").unwrap();
        assert!(leaf.transform.is_invertible());

        let test_pts = [
            Vec2::ZERO,
            Vec2::new(50.0, 50.0),
            Vec2::new(-30.0, 80.0),
            Vec2::new(1920.0, 1080.0),
        ];

        for local_pt in test_pts {
            let world_pt = leaf.local_to_world_point(local_pt);
            let recovered = leaf.world_to_local_point(world_pt).unwrap();
            assert!(
                local_pt.distance_to(recovered) < 1e-3,
                "Failed roundtrip for {:?} at frame {frame_idx}: got {:?}",
                local_pt,
                recovered
            );
        }
    }

    // Midpoint (frame 15) verification: Bezier ease-in-out symmetry
    let stack_15 = evaluator.evaluate(&graph, &TimeCode::from_frames(15, fps));
    let t0_15 = stack_15.get_transform("node_0").unwrap();
    // Midpoint of (100, 100) and (500, 300) with symmetric ease-in-out should be exactly (300, 200)
    assert!((t0_15.position.x - 300.0).abs() < 1.0);
    assert!((t0_15.position.y - 200.0).abs() < 1.0);

    let t1_15 = stack_15.get_transform("node_1").unwrap();
    // Midpoint of 0 and 90 deg rotation with symmetric ease-in-out should be 45 deg
    assert!((t1_15.rotation - 45.0).abs() < 1.0);
}

// 3. Track matte masking (Alpha, AlphaInverted, Luma, LumaInverted) with both explicit layer IDs
// and adjacent stack layers, verifying matte consumption and preservation under soloing.
#[test]
fn test_track_matte_masking_all_modes_and_soloing() {
    let fps = 30.0;
    let mut comp = Composition::hd_1080p_30fps("comp_mattes", "Matte Modes & Solo", 5.0);
    let tc0 = TimeCode::from_frames(0, fps);
    let tc150 = TimeCode::from_frames(150, fps);

    // Visual stack order in timeline (index 0 is top):
    // [0] matte_alpha_adj (Adjacent matte source for Layer Alpha)
    // [1] layer_alpha     (Target: Alpha Matte using adjacent above)
    // [2] matte_alphainv  (Adjacent matte source for Layer AlphaInv)
    // [3] layer_alphainv  (Target: AlphaInverted Matte using adjacent above)
    // [4] matte_luma_src  (Explicit matte source for Layer Luma)
    // [5] unrelated_layer (Decouples adjacency)
    // [6] layer_luma      (Target: Luma Matte using explicit matte_layer_id = "matte_luma_src")
    // [7] matte_lumainv_src (Explicit matte source for Layer LumaInv)
    // [8] layer_lumainv   (Target: LumaInverted Matte using explicit matte_layer_id = "matte_lumainv_src")
    // [9] background_solid

    let m_alpha_adj = project::Layer::solid("m_alpha_adj", "Alpha Mask", Color::WHITE, 200, 200, tc0, tc150);
    let mut l_alpha = project::Layer::solid("l_alpha", "Alpha Target", Color::RED, 1920, 1080, tc0, tc150);
    l_alpha.set_matte(TrackMatteMode::Alpha, None::<String>); // adjacent

    let m_alphainv_adj = project::Layer::solid("m_alphainv_adj", "AlphaInv Mask", Color::WHITE, 200, 200, tc0, tc150);
    let mut l_alphainv = project::Layer::solid("l_alphainv", "AlphaInv Target", Color::BLUE, 1920, 1080, tc0, tc150);
    l_alphainv.set_matte(TrackMatteMode::AlphaInverted, None::<String>); // adjacent

    let m_luma_src = project::Layer::solid("m_luma_src", "Luma Mask", Color::WHITE, 200, 200, tc0, tc150);
    let unrelated = project::Layer::solid("unrelated", "Spacer", Color::rgba(0.5, 0.5, 0.5, 1.0), 100, 100, tc0, tc150);
    let mut l_luma = project::Layer::solid("l_luma", "Luma Target", Color::GREEN, 1920, 1080, tc0, tc150);
    l_luma.set_matte(TrackMatteMode::Luma, Some("m_luma_src")); // explicit

    let m_lumainv_src = project::Layer::solid("m_lumainv_src", "LumaInv Mask", Color::WHITE, 200, 200, tc0, tc150);
    let mut l_lumainv = project::Layer::solid("l_lumainv", "LumaInv Target", Color::rgba(1.0, 1.0, 0.0, 1.0), 1920, 1080, tc0, tc150);
    l_lumainv.set_matte(TrackMatteMode::LumaInverted, Some("m_lumainv_src")); // explicit

    let bg = project::Layer::solid("bg", "Background", Color::BLACK, 1920, 1080, tc0, tc150);

    comp.add_layer(m_alpha_adj).unwrap();
    comp.add_layer(l_alpha).unwrap();
    comp.add_layer(m_alphainv_adj).unwrap();
    comp.add_layer(l_alphainv).unwrap();
    comp.add_layer(m_luma_src).unwrap();
    comp.add_layer(unrelated).unwrap();
    comp.add_layer(l_luma).unwrap();
    comp.add_layer(m_lumainv_src).unwrap();
    comp.add_layer(l_lumainv).unwrap();
    comp.add_layer(bg).unwrap();

    let graph = SceneGraph::from_composition(&comp).unwrap();

    // 1. Standard evaluation with matte consumption enabled (default)
    let evaluator = LayerStackEvaluator::new();
    let stack = evaluator.evaluate(&graph, &tc0);

    // Verify track matte pairings
    let el_alpha = stack.get_layer("l_alpha").unwrap();
    assert_eq!(el_alpha.matte_mode, TrackMatteMode::Alpha);
    assert_eq!(el_alpha.matte_source_id.as_deref(), Some("m_alpha_adj"));
    assert!(stack.get_layer("m_alpha_adj").unwrap().is_matte_source);

    let el_alphainv = stack.get_layer("l_alphainv").unwrap();
    assert_eq!(el_alphainv.matte_mode, TrackMatteMode::AlphaInverted);
    assert_eq!(el_alphainv.matte_source_id.as_deref(), Some("m_alphainv_adj"));
    assert!(stack.get_layer("m_alphainv_adj").unwrap().is_matte_source);

    let el_luma = stack.get_layer("l_luma").unwrap();
    assert_eq!(el_luma.matte_mode, TrackMatteMode::Luma);
    assert_eq!(el_luma.matte_source_id.as_deref(), Some("m_luma_src"));
    assert!(stack.get_layer("m_luma_src").unwrap().is_matte_source);

    let el_lumainv = stack.get_layer("l_lumainv").unwrap();
    assert_eq!(el_lumainv.matte_mode, TrackMatteMode::LumaInverted);
    assert_eq!(el_lumainv.matte_source_id.as_deref(), Some("m_lumainv_src"));
    assert!(stack.get_layer("m_lumainv_src").unwrap().is_matte_source);

    // Verify matte consumption: matte sources are NOT in render_list
    assert!(!stack.render_list.contains(&"m_alpha_adj".to_string()));
    assert!(!stack.render_list.contains(&"m_alphainv_adj".to_string()));
    assert!(!stack.render_list.contains(&"m_luma_src".to_string()));
    assert!(!stack.render_list.contains(&"m_lumainv_src".to_string()));

    // But targets, spacer, and background ARE in render_list (in painter's bottom-up order)
    assert_eq!(
        stack.render_list,
        vec!["bg", "l_lumainv", "l_luma", "unrelated", "l_alphainv", "l_alpha"]
    );

    // 2. Evaluation with consume_matte_sources = false
    let non_consuming = LayerStackEvaluator::new().with_consume_matte_sources(false);
    let stack_nc = non_consuming.evaluate(&graph, &tc0);
    assert_eq!(stack_nc.render_count(), 10);
    assert!(stack_nc.render_list.contains(&"m_alpha_adj".to_string()));

    // 3. Soloing behavior:
    // Case A: Soloing masked layer "l_luma" MUST preserve "m_luma_src" as eligible matte source
    let mut comp_solo_target = comp.clone();
    comp_solo_target.get_layer_mut("l_luma").unwrap().set_solo(true);
    let graph_solo = SceneGraph::from_composition(&comp_solo_target).unwrap();
    let stack_solo = evaluator.evaluate(&graph_solo, &tc0);

    assert!(stack_solo.has_solo());
    assert_eq!(stack_solo.render_list, vec!["l_luma"]); // only l_luma renders

    // Verify that matte source "m_luma_src" was preserved (is_visible = true)
    let solo_matte = stack_solo.get_layer("m_luma_src").unwrap();
    assert!(solo_matte.is_active);
    assert!(solo_matte.is_visible); // Preserved for matte sampling!

    // Other non-solo layers are suppressed (is_visible = false)
    assert!(!stack_solo.get_layer("l_alpha").unwrap().is_visible);
    assert!(!stack_solo.get_layer("unrelated").unwrap().is_visible);
    assert!(!stack_solo.get_layer("bg").unwrap().is_visible);

    // Case B: Soloing "unrelated" suppresses both l_luma and m_luma_src
    let mut comp_solo_other = comp.clone();
    comp_solo_other.get_layer_mut("unrelated").unwrap().set_solo(true);
    let graph_other = SceneGraph::from_composition(&comp_solo_other).unwrap();
    let stack_other = evaluator.evaluate(&graph_other, &tc0);

    assert_eq!(stack_other.render_list, vec!["unrelated"]);
    assert!(!stack_other.get_layer("l_luma").unwrap().is_visible);
    assert!(!stack_other.get_layer("m_luma_src").unwrap().is_visible);
}

// 4. Multi-level nested pre-comps (Root -> PreComp1 -> PreComp2) with time stretching
// (slow-mo, double speed, reverse), start offsets, and animated time remapping.
#[test]
fn test_multi_level_nested_precomps_time_stretch_and_remapping() {
    let mut project = Project::with_defaults("Nested Time & Speed Project");
    let fps = 30.0;
    let tc0 = TimeCode::from_frames(0, fps);
    let tc60 = TimeCode::from_frames(60, fps);
    let tc120 = TimeCode::from_frames(120, fps);
    let tc300 = TimeCode::from_frames(300, fps);

    // PreComp2 (innermost leaf comp, duration 60 frames = 2.0s)
    let mut comp_leaf = Composition::new("comp_leaf", "Innermost Leaf", 400, 400, fps, tc60);
    let leaf_solid = project::Layer::solid("leaf_solid", "Leaf Solid", Color::WHITE, 200, 200, tc0, tc60);
    comp_leaf.add_layer(leaf_solid).unwrap();

    // PreComp1 (middle comp):
    // Nests comp_leaf with:
    // - 50% slow-motion (time_stretch = 0.5)
    // - start_offset = 10 frames
    // - LoopMode::PingPong
    let mut comp_mid = Composition::new("comp_mid", "Middle Comp", 800, 800, fps, tc120);
    let mid_nest = project::Layer::nested_composition("nest_leaf", "Leaf Precomp", "comp_leaf", tc0, tc120)
        .with_time_stretch(0.5)
        .with_start_offset(TimeCode::from_frames(10, fps))
        .with_loop_mode(LoopMode::PingPong);
    comp_mid.add_layer(mid_nest).unwrap();

    // Root Comp:
    // 1. Layer 1 nests comp_mid with animated time remapping:
    //    Frame 0 -> 0.0s (0 frames)
    //    Frame 30 -> 2.0s (60 frames)
    //    Frame 60 -> 1.0s (30 frames)
    let mut remap_prop = Property::new("Time Remap", 0.0);
    remap_prop.add_keyframe(Keyframe::linear(tc0, 0.0));
    remap_prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(30, fps), 2.0));
    remap_prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(60, fps), 1.0));

    let mut root_comp = Composition::hd_1080p_30fps("comp_root", "Root Comp", 10.0);
    let root_nest_mid = project::Layer::nested_composition("nest_mid", "Mid Precomp", "comp_mid", tc0, tc300)
        .with_time_remapping(remap_prop);

    // 2. Layer 2 nests comp_leaf directly with 200% double speed and LoopMode::Once
    let root_nest_fast = project::Layer::nested_composition("nest_fast", "Fast Precomp", "comp_leaf", tc0, tc300)
        .with_time_stretch(2.0)
        .with_loop_mode(LoopMode::Once);

    // 3. Layer 3 nests comp_leaf directly with reverse playback (-1.0x) and offset 50 frames
    let root_nest_rev = project::Layer::nested_composition("nest_rev", "Reverse Precomp", "comp_leaf", tc0, tc300)
        .with_time_stretch(-1.0)
        .with_start_offset(TimeCode::from_frames(50, fps));

    root_comp.add_layer(root_nest_mid).unwrap();
    root_comp.add_layer(root_nest_fast).unwrap();
    root_comp.add_layer(root_nest_rev).unwrap();

    project.add_composition(root_comp).unwrap();
    project.add_composition(comp_mid).unwrap();
    project.add_composition(comp_leaf).unwrap();

    let evaluator = LayerStackEvaluator::new();

    // 1. Root Frame 0:
    // - nest_mid: remapping = 0.0s -> comp_mid at frame 0.
    //   In comp_mid, nest_leaf at frame 0: 0 * 0.5 + 10 = 10 frames.
    // - nest_fast: 0 * 2.0 = 0 frames.
    // - nest_rev: 0 * -1.0 + 50 = 50 frames.
    let s0 = evaluator
        .evaluate_composition(&project, "comp_root", &tc0)
        .expect("Evaluate frame 0");

    let mid_eval_0 = s0.get_layer("nest_mid").unwrap().nested_evaluation().unwrap();
    assert_eq!(mid_eval_0.resolved_time.frames(), 0);

    let leaf_eval_0 = mid_eval_0
        .inner_stack
        .get_layer("nest_leaf")
        .unwrap()
        .nested_evaluation()
        .unwrap();
    assert_eq!(leaf_eval_0.resolved_time.frames(), 10);

    let fast_eval_0 = s0.get_layer("nest_fast").unwrap().nested_evaluation().unwrap();
    assert_eq!(fast_eval_0.resolved_time.frames(), 0);

    let rev_eval_0 = s0.get_layer("nest_rev").unwrap().nested_evaluation().unwrap();
    assert_eq!(rev_eval_0.resolved_time.frames(), 50);

    // 2. Root Frame 30:
    // - nest_mid: remapping = 2.0s -> 60 frames in comp_mid.
    //   In comp_mid, nest_leaf: 60 * 0.5 + 10 = 40 frames.
    // - nest_fast: 30 * 2.0 = 60 frames.
    // - nest_rev: 30 * -1.0 + 50 = 20 frames.
    let s30 = evaluator
        .evaluate_composition(&project, "comp_root", &TimeCode::from_frames(30, fps))
        .expect("Evaluate frame 30");

    let mid_eval_30 = s30.get_layer("nest_mid").unwrap().nested_evaluation().unwrap();
    assert_eq!(mid_eval_30.resolved_time.frames(), 60);

    let leaf_eval_30 = mid_eval_30
        .inner_stack
        .get_layer("nest_leaf")
        .unwrap()
        .nested_evaluation()
        .unwrap();
    assert_eq!(leaf_eval_30.resolved_time.frames(), 40);

    let fast_eval_30 = s30.get_layer("nest_fast").unwrap().nested_evaluation().unwrap();
    assert_eq!(fast_eval_30.resolved_time.frames(), 60);

    let rev_eval_30 = s30.get_layer("nest_rev").unwrap().nested_evaluation().unwrap();
    assert_eq!(rev_eval_30.resolved_time.frames(), 20);

    // 3. Root Frame 60:
    // - nest_mid: remapping = 1.0s -> 30 frames in comp_mid.
    //   In comp_mid, nest_leaf: 30 * 0.5 + 10 = 25 frames.
    let s60 = evaluator
        .evaluate_composition(&project, "comp_root", &TimeCode::from_frames(60, fps))
        .expect("Evaluate frame 60");

    let mid_eval_60 = s60.get_layer("nest_mid").unwrap().nested_evaluation().unwrap();
    assert_eq!(mid_eval_60.resolved_time.frames(), 30);

    let leaf_eval_60 = mid_eval_60
        .inner_stack
        .get_layer("nest_leaf")
        .unwrap()
        .nested_evaluation()
        .unwrap();
    assert_eq!(leaf_eval_60.resolved_time.frames(), 25);
}

// 5. Timecode evaluation over full timeline spans, checking layer activation boundaries
// (`[in_point, out_point)`), opacity fades, and composite render list order (painter's algorithm).
#[test]
fn test_timecode_evaluation_over_full_timeline_span() {
    let fps = 30.0;
    let mut comp = Composition::hd_1080p_30fps("comp_span", "Timeline Span Test", 10.0);
    let total_frames = 300;

    // Layer 0: Background solid spanning full duration [0, 300)
    let bg = project::Layer::solid(
        "bg",
        "Background",
        Color::BLACK,
        1920,
        1080,
        TimeCode::from_frames(0, fps),
        TimeCode::from_frames(total_frames, fps),
    );

    // Layer 1: Intro title spanning [0, 90)
    let intro = project::Layer::text(
        "intro",
        "Intro",
        "Intro Text",
        "Inter",
        48.0,
        Color::WHITE,
        TimeCode::from_frames(0, fps),
        TimeCode::from_frames(90, fps),
    );

    // Layer 2: Main feature spanning [60, 240) with opacity keyframes:
    // [60, 90): fade in 0 -> 100%
    // [90, 210): hold 100%
    // [210, 240): fade out 100% -> 0%
    let mut feature = project::Layer::solid(
        "feature",
        "Feature",
        Color::RED,
        800,
        600,
        TimeCode::from_frames(60, fps),
        TimeCode::from_frames(240, fps),
    );
    feature.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(60, fps), 0.0));
    feature.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(90, fps), 100.0));
    feature.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(210, fps), 100.0));
    feature.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(240, fps), 0.0));

    // Layer 3: Outro title spanning [210, 300)
    let outro = project::Layer::text(
        "outro",
        "Outro",
        "Outro Text",
        "Inter",
        48.0,
        Color::WHITE,
        TimeCode::from_frames(210, fps),
        TimeCode::from_frames(total_frames, fps),
    );

    comp.add_layer(bg).unwrap();
    comp.add_layer(intro).unwrap();
    comp.add_layer(feature).unwrap();
    comp.add_layer(outro).unwrap();

    let graph = SceneGraph::from_composition(&comp).unwrap();
    let evaluator = LayerStackEvaluator::new();

    // Test exact frame boundaries:
    // Frame 0: bg, intro active. feature and outro inactive.
    let s0 = evaluator.evaluate(&graph, &TimeCode::from_frames(0, fps));
    assert!(s0.get_layer("bg").unwrap().is_active);
    assert!(s0.get_layer("intro").unwrap().is_active);
    assert!(!s0.get_layer("feature").unwrap().is_active);
    assert!(!s0.get_layer("outro").unwrap().is_active);
    // Composite order: intro (1) renders over bg (0)
    assert_eq!(s0.render_list, vec!["intro", "bg"]);

    // Frame 59 (last frame before feature in_point):
    let s59 = evaluator.evaluate(&graph, &TimeCode::from_frames(59, fps));
    assert!(!s59.get_layer("feature").unwrap().is_active);

    // Frame 60 (exact feature in_point):
    // feature is active, but opacity is 0.0 so effective_opacity == 0.0 and is_rendered() == false
    let s60 = evaluator.evaluate(&graph, &TimeCode::from_frames(60, fps));
    let feat_60 = s60.get_layer("feature").unwrap();
    assert!(feat_60.is_active);
    assert_eq!(feat_60.local_opacity, 0.0);
    assert_eq!(feat_60.effective_opacity, 0.0);
    assert!(!feat_60.is_rendered());

    // Frame 75 (midpoint of fade-in): opacity is 50%
    let s75 = evaluator.evaluate(&graph, &TimeCode::from_frames(75, fps));
    let feat_75 = s75.get_layer("feature").unwrap();
    assert!(feat_75.is_active);
    assert!((feat_75.local_opacity - 50.0).abs() < 1e-4);
    assert!((feat_75.effective_opacity - 0.5).abs() < 1e-4);
    assert!(feat_75.is_rendered());
    // Both intro and feature are active; composite order: feature (2) -> intro (1) -> bg (0)
    assert_eq!(s75.render_list, vec!["feature", "intro", "bg"]);

    // Frame 89 (last frame of intro): intro active
    let s89 = evaluator.evaluate(&graph, &TimeCode::from_frames(89, fps));
    assert!(s89.get_layer("intro").unwrap().is_active);

    // Frame 90 (exact out_point of intro): intro is INACTIVE [0, 90)
    let s90 = evaluator.evaluate(&graph, &TimeCode::from_frames(90, fps));
    assert!(!s90.get_layer("intro").unwrap().is_active);
    assert_eq!(s90.render_list, vec!["feature", "bg"]);

    // Frame 150 (midway through span): only feature and bg active
    let s150 = evaluator.evaluate(&graph, &TimeCode::from_frames(150, fps));
    assert_eq!(s150.active_count(), 2);
    assert_eq!(s150.render_list, vec!["feature", "bg"]);

    // Frame 225 (midpoint of feature fade-out, outro active):
    let s225 = evaluator.evaluate(&graph, &TimeCode::from_frames(225, fps));
    let feat_225 = s225.get_layer("feature").unwrap();
    assert!((feat_225.local_opacity - 50.0).abs() < 1e-4);
    assert!(s225.get_layer("outro").unwrap().is_active);
    // Composite order: outro (3) -> feature (2) -> bg (0)
    assert_eq!(s225.render_list, vec!["outro", "feature", "bg"]);

    // Frame 239 (last frame of feature):
    let s239 = evaluator.evaluate(&graph, &TimeCode::from_frames(239, fps));
    assert!(s239.get_layer("feature").unwrap().is_active);

    // Frame 240 (exact out_point of feature): feature is INACTIVE [60, 240)
    let s240 = evaluator.evaluate(&graph, &TimeCode::from_frames(240, fps));
    assert!(!s240.get_layer("feature").unwrap().is_active);
    assert_eq!(s240.render_list, vec!["outro", "bg"]);

    // Frame 299 (last active frame of comp): outro and bg active
    let s299 = evaluator.evaluate(&graph, &TimeCode::from_frames(299, fps));
    assert!(s299.get_layer("bg").unwrap().is_active);
    assert!(s299.get_layer("outro").unwrap().is_active);

    // Frame 300 (exact comp out_point): all layers inactive
    let s300 = evaluator.evaluate(&graph, &TimeCode::from_frames(300, fps));
    assert_eq!(s300.active_count(), 0);
    assert!(s300.render_list.is_empty());
}

// 6. Full tree point mapping: mapping a local point inside the innermost layer of a nested pre-comp
// all the way out to root composition viewport coordinates across animated parent/nesting transforms.
#[test]
fn test_full_tree_point_and_bounds_mapping() {
    let mut project = Project::with_defaults("Full Tree Point Mapping Project");
    let fps = 30.0;
    let tc0 = TimeCode::from_frames(0, fps);
    let tc120 = TimeCode::from_frames(120, fps);
    let tc150 = TimeCode::from_frames(150, fps);

    // Comp C (Innermost, 500x500):
    // Contains "c_parent" with keyframed transform animations (Position, Rotation, Scale, Anchor Point)
    // Contains "c_leaf" parented to "c_parent" with keyframed transform animations
    let mut comp_c = Composition::new("comp_c", "Comp C", 500, 500, fps, tc150);
    let mut c_parent = project::Layer::solid("c_parent", "C Parent", Color::BLUE, 100, 100, tc0, tc150);
    c_parent.transform.position.add_keyframe(Keyframe::bezier(
        tc0,
        Vec2::new(60.0, 40.0),
        None,
        Some(KeyframeTangent::ease_in_out_out()),
    ));
    c_parent.transform.position.add_keyframe(Keyframe::bezier(
        tc120,
        Vec2::new(140.0, 90.0),
        Some(KeyframeTangent::ease_in_out_in()),
        None,
    ));
    c_parent.transform.rotation.add_keyframe(Keyframe::linear(tc0, 30.0));
    c_parent.transform.rotation.add_keyframe(Keyframe::linear(tc120, 120.0));
    c_parent.transform.scale.set_value(Vec2::new(120.0, 120.0));
    c_parent.transform.anchor_point.set_value(Vec2::new(10.0, 10.0));

    let mut c_leaf = project::Layer::solid("c_leaf", "C Leaf", Color::GREEN, 50, 50, tc0, tc150);
    c_leaf.set_parent(Some("c_parent"));
    c_leaf.transform.position.add_keyframe(Keyframe::linear(tc0, Vec2::new(25.0, -15.0)));
    c_leaf.transform.position.add_keyframe(Keyframe::linear(tc120, Vec2::new(50.0, 20.0)));
    c_leaf.transform.rotation.add_keyframe(Keyframe::linear(tc0, -15.0));
    c_leaf.transform.rotation.add_keyframe(Keyframe::linear(tc120, 45.0));
    c_leaf.transform.scale.set_value(Vec2::new(80.0, 150.0));
    c_leaf.transform.anchor_point.set_value(Vec2::new(5.0, 5.0));

    comp_c.add_layer(c_parent).unwrap();
    comp_c.add_layer(c_leaf).unwrap();

    // Comp B (Middle, 800x800):
    // Nests comp_c with animated position, rotation, and scale
    let mut comp_b = Composition::new("comp_b", "Comp B", 800, 800, fps, tc150);
    let mut b_nest = project::Layer::nested_composition("b_nest_c", "Nested C", "comp_c", tc0, tc150);
    b_nest.transform.position.add_keyframe(Keyframe::linear(tc0, Vec2::new(150.0, 200.0)));
    b_nest.transform.position.add_keyframe(Keyframe::linear(tc120, Vec2::new(220.0, 180.0)));
    b_nest.transform.rotation.add_keyframe(Keyframe::linear(tc0, 45.0));
    b_nest.transform.rotation.add_keyframe(Keyframe::linear(tc120, 90.0));
    b_nest.transform.scale.set_value(Vec2::new(150.0, 100.0));
    b_nest.transform.anchor_point.set_value(Vec2::new(50.0, 50.0));
    comp_b.add_layer(b_nest).unwrap();

    // Comp A (Root, 1920x1080):
    // Nests comp_b with animated position, rotation, and scale
    let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Comp A", 5.0);
    let mut a_nest = project::Layer::nested_composition("a_nest_b", "Nested B", "comp_b", tc0, tc150);
    a_nest.transform.position.add_keyframe(Keyframe::bezier(
        tc0,
        Vec2::new(300.0, 150.0),
        None,
        Some(KeyframeTangent::ease_in_out_out()),
    ));
    a_nest.transform.position.add_keyframe(Keyframe::bezier(
        tc120,
        Vec2::new(450.0, 300.0),
        Some(KeyframeTangent::ease_in_out_in()),
        None,
    ));
    a_nest.transform.rotation.add_keyframe(Keyframe::linear(tc0, -20.0));
    a_nest.transform.rotation.add_keyframe(Keyframe::linear(tc120, 40.0));
    a_nest.transform.scale.set_value(Vec2::new(110.0, 90.0));
    a_nest.transform.anchor_point.set_value(Vec2::new(100.0, 100.0));
    comp_a.add_layer(a_nest).unwrap();

    project.add_composition(comp_a).unwrap();
    project.add_composition(comp_b).unwrap();
    project.add_composition(comp_c).unwrap();

    let evaluator = LayerStackEvaluator::new();

    let local_points = [
        Vec2::ZERO,
        Vec2::new(5.0, 5.0),   // Anchor point
        Vec2::new(50.0, 50.0), // Opposite corner
        Vec2::new(-20.0, 35.0),
        Vec2::new(25.0, 10.0),
    ];

    let mut mapped_origin_per_frame = Vec::new();

    // Evaluate across multiple animated timeline positions
    for frame_idx in [0, 30, 60, 90, 120] {
        let t = TimeCode::from_frames(frame_idx, fps);
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &t)
            .unwrap_or_else(|e| panic!("Evaluate comp_a at frame {frame_idx}: {e}"));

        // Retrieve leaf layer flattened representation
        let flat_leaf = stack_a
            .get_flattened_layer("c_leaf")
            .expect("Leaf exists in flattened render list");

        assert_eq!(flat_leaf.layer_path, vec!["a_nest_b", "b_nest_c", "c_leaf"]);
        assert_eq!(flat_leaf.nesting_depth, 2);

        // Verify deep layer root matrix equals flattened layer root world matrix
        let deep_matrix = stack_a
            .deep_layer_root_matrix(&["a_nest_b", "b_nest_c", "c_leaf"])
            .expect("Deep layer root matrix");
        assert!(
            flat_leaf.root_world_matrix.approx_eq(&deep_matrix, 1e-4),
            "deep_matrix != root_world_matrix at frame {frame_idx}"
        );

        // Verify exact hierarchical matrix concatenation across nesting boundaries:
        // M_root = M_root(a_nest_b) * M_comp_b(b_nest_c) * M_comp_c(c_parent) * M_local(c_leaf)
        let a_eval = stack_a.get_layer("a_nest_b").unwrap();
        let b_eval = a_eval.nested_evaluation().unwrap();
        let b_layer = b_eval.inner_stack.get_layer("b_nest_c").unwrap();
        let c_eval = b_layer.nested_evaluation().unwrap();
        let c_parent_eval = c_eval.inner_stack.get_layer("c_parent").unwrap();
        let c_leaf_eval = c_eval.inner_stack.get_layer("c_leaf").unwrap();

        let expected_concatenated = a_eval.world_matrix()
            * b_layer.world_matrix()
            * c_parent_eval.world_matrix()
            * c_leaf_eval.local_matrix();

        assert!(
            flat_leaf.root_world_matrix.approx_eq(&expected_concatenated, 1e-3),
            "Concatenation mismatch at frame {frame_idx}"
        );

        // Test bidirectional point mapping across animated parent/nesting transforms
        for &local_pt in &local_points {
            let root_pt = flat_leaf.local_to_root_point(local_pt);
            let recovered = flat_leaf
                .root_to_local_point(root_pt)
                .expect("Invertible transform");

            assert!(
                local_pt.distance_to(recovered) < 1e-3,
                "Point mapping roundtrip failed at frame {frame_idx} for {:?}: got {:?}",
                local_pt,
                recovered
            );
        }

        // Verify root bounding box dynamically encloses all 4 corners of the 50x50 solid
        let root_bounds = flat_leaf.root_bounds(50.0, 50.0);
        assert!(root_bounds.width() > 0.0);
        assert!(root_bounds.height() > 0.0);

        let corners = [
            Vec2::new(0.0, 0.0),
            Vec2::new(50.0, 0.0),
            Vec2::new(50.0, 50.0),
            Vec2::new(0.0, 50.0),
        ];
        for corner in corners {
            let root_corner = flat_leaf.local_to_root_point(corner);
            let eps = 1e-2;
            assert!(
                root_corner.x >= root_bounds.min.x - eps
                    && root_corner.x <= root_bounds.max.x + eps
                    && root_corner.y >= root_bounds.min.y - eps
                    && root_corner.y <= root_bounds.max.y + eps,
                "Root bounds {:?} does not enclose corner {:?} -> root {:?} at frame {frame_idx}",
                root_bounds,
                corner,
                root_corner
            );
        }

        let root_origin = flat_leaf.local_to_root_point(Vec2::ZERO);
        mapped_origin_per_frame.push(root_origin);
    }

    // Verify that coordinates actually changed over time due to animated transforms
    for i in 1..mapped_origin_per_frame.len() {
        assert!(
            mapped_origin_per_frame[i - 1].distance_to(mapped_origin_per_frame[i]) > 5.0,
            "Mapped points did not animate between evaluation frames: {:?} vs {:?}",
            mapped_origin_per_frame[i - 1],
            mapped_origin_per_frame[i]
        );
    }
}

// 7. Robust error handling: cycles in parenting, circular nested compositions,
// missing layer/comp references, out-of-bounds frame requests.
#[test]
fn test_robust_error_handling_and_boundary_conditions() {
    let fps = 30.0;
    let tc0 = TimeCode::from_frames(0, fps);
    let tc150 = TimeCode::from_frames(150, fps);

    // 1. Cycles in parenting: 3-layer cycle A -> B -> C -> A
    let mut comp_cycle = Composition::hd_1080p_30fps("comp_cyc", "Cycle", 5.0);
    let mut la = project::Layer::solid("la", "A", Color::RED, 10, 10, tc0, tc150);
    let mut lb = project::Layer::solid("lb", "B", Color::GREEN, 10, 10, tc0, tc150);
    let mut lc = project::Layer::solid("lc", "C", Color::BLUE, 10, 10, tc0, tc150);
    la.set_parent(Some("lb"));
    lb.set_parent(Some("lc"));
    lc.set_parent(Some("la"));
    comp_cycle.add_layer(la).unwrap();
    comp_cycle.add_layer(lb).unwrap();
    comp_cycle.add_layer(lc).unwrap();

    let err_parent_cycle = SceneGraph::from_composition(&comp_cycle).unwrap_err();
    assert!(matches!(
        err_parent_cycle,
        SceneGraphError::ParentCycleDetected { .. }
    ));

    // 2. Self-parenting
    let mut comp_self = Composition::hd_1080p_30fps("comp_self", "Self Parent", 5.0);
    let mut l_self = project::Layer::solid("l_self", "Self", Color::RED, 10, 10, tc0, tc150);
    l_self.set_parent(Some("l_self"));
    comp_self.add_layer(l_self).unwrap();
    assert!(matches!(
        SceneGraph::from_composition(&comp_self),
        Err(SceneGraphError::SelfParenting(ref id)) if id == "l_self"
    ));

    // 3. Missing parent reference
    let mut comp_miss_p = Composition::hd_1080p_30fps("comp_mp", "Missing Parent", 5.0);
    let mut l_orphan = project::Layer::solid("orphan", "Orphan", Color::RED, 10, 10, tc0, tc150);
    l_orphan.set_parent(Some("non_existent_parent_id"));
    comp_miss_p.add_layer(l_orphan).unwrap();
    assert!(matches!(
        SceneGraph::from_composition(&comp_miss_p),
        Err(SceneGraphError::ParentNotFound { .. })
    ));

    // 4. Missing layer/node reference in SceneGraph queries
    let mut comp_valid = Composition::hd_1080p_30fps("valid", "Valid", 5.0);
    comp_valid
        .add_layer(project::Layer::solid("s", "S", Color::RED, 100, 100, tc0, tc150))
        .unwrap();
    let graph_valid = SceneGraph::from_composition(&comp_valid).unwrap();

    let err_missing_node = graph_valid.get_evaluated_transform("ghost_node").unwrap_err();
    assert!(matches!(
        err_missing_node,
        SceneGraphError::NodeNotFound(ref id) if id == "ghost_node"
    ));
    let err_missing_at = graph_valid.get_evaluated_transform_at("ghost_node", &tc0).unwrap_err();
    assert!(matches!(
        err_missing_at,
        SceneGraphError::NodeNotFound(ref id) if id == "ghost_node"
    ));

    // 5. Circular nested compositions: Comp 1 -> Comp 2 -> Comp 3 -> Comp 1
    let mut project_nested_cycle = Project::with_defaults("Nested Cycle Project");
    let mut c1 = Composition::hd_1080p_30fps("c1", "Comp 1", 5.0);
    let mut c2 = Composition::hd_1080p_30fps("c2", "Comp 2", 5.0);
    let mut c3 = Composition::hd_1080p_30fps("c3", "Comp 3", 5.0);

    c1.add_layer(project::Layer::nested_composition("n2", "Nests C2", "c2", tc0, tc150)).unwrap();
    c2.add_layer(project::Layer::nested_composition("n3", "Nests C3", "c3", tc0, tc150)).unwrap();
    c3.add_layer(project::Layer::nested_composition("n1", "Nests C1", "c1", tc0, tc150)).unwrap();

    project_nested_cycle.add_composition(c1).unwrap();
    project_nested_cycle.add_composition(c2).unwrap();
    project_nested_cycle.add_composition(c3).unwrap();

    let evaluator = LayerStackEvaluator::new();
    let err_comp_cycle = evaluator
        .evaluate_composition(&project_nested_cycle, "c1", &tc0)
        .unwrap_err();
    assert!(matches!(
        err_comp_cycle,
        SceneGraphError::CircularNestedComposition { ref composition_id, .. } if composition_id == "c1"
    ));

    // 6. Missing composition reference
    let mut project_missing = Project::with_defaults("Missing Comp Proj");
    let mut c_root = Composition::hd_1080p_30fps("c_root", "Root", 5.0);
    c_root
        .add_layer(project::Layer::nested_composition("n_ghost", "Ghost", "ghost_comp", tc0, tc150))
        .unwrap();
    project_missing.add_composition(c_root).unwrap();

    let err_missing_comp = evaluator
        .evaluate_composition(&project_missing, "c_root", &tc0)
        .unwrap_err();
    assert!(matches!(
        err_missing_comp,
        SceneGraphError::CompositionNotFound(ref id) if id == "ghost_comp"
    ));

    // 7. Max nesting depth exceeded
    let strict_eval = LayerStackEvaluator::new().with_max_nesting_depth(2);
    let mut proj_deep = Project::with_defaults("Deep Limit Proj");
    let mut d4 = Composition::hd_1080p_30fps("d4", "D4", 5.0);
    d4.add_layer(project::Layer::solid("d_leaf", "Leaf", Color::WHITE, 10, 10, tc0, tc150)).unwrap();
    let mut d3 = Composition::hd_1080p_30fps("d3", "D3", 5.0);
    d3.add_layer(project::Layer::nested_composition("n4", "N4", "d4", tc0, tc150)).unwrap();
    let mut d2 = Composition::hd_1080p_30fps("d2", "D2", 5.0);
    d2.add_layer(project::Layer::nested_composition("n3", "N3", "d3", tc0, tc150)).unwrap();
    let mut d1 = Composition::hd_1080p_30fps("d1", "D1", 5.0);
    d1.add_layer(project::Layer::nested_composition("n2", "N2", "d2", tc0, tc150)).unwrap();

    proj_deep.add_composition(d1).unwrap();
    proj_deep.add_composition(d2).unwrap();
    proj_deep.add_composition(d3).unwrap();
    proj_deep.add_composition(d4).unwrap();

    let err_depth = strict_eval.evaluate_composition(&proj_deep, "d1", &tc0).unwrap_err();
    assert!(matches!(
        err_depth,
        SceneGraphError::MaxNestingDepthExceeded { depth: 3, max_depth: 2 }
    ));

    // 8. Missing track matte source layer reference: graceful fallback without panics
    let mut comp_broken_matte = Composition::hd_1080p_30fps("comp_bm", "Broken Matte", 5.0);
    let mut l_target = project::Layer::solid("l_target", "Target", Color::RED, 100, 100, tc0, tc150);
    l_target.set_matte(TrackMatteMode::Alpha, Some("phantom_matte_source"));
    comp_broken_matte.add_layer(l_target).unwrap();
    let graph_bm = SceneGraph::from_composition(&comp_broken_matte).unwrap();
    let stack_bm = evaluator.evaluate(&graph_bm, &tc0);
    let eval_target = stack_bm.get_layer("l_target").unwrap();
    assert_eq!(eval_target.matte_source_id, None);
    assert!(eval_target.is_active);
    assert!(eval_target.is_visible);
    assert_eq!(stack_bm.render_list, vec!["l_target"]);

    // Missing layer deep lookup query returns None safely
    assert!(stack_bm.get_layer("ghost_layer").is_none());
    assert!(stack_bm.get_layer_deep(&["l_target", "ghost_layer"]).is_none());
    assert!(stack_bm.deep_layer_root_matrix(&["l_target", "ghost_layer"]).is_none());

    // 9. Out-of-bounds frame requests & subframe boundaries:
    // A: Negative frame number
    let neg_time = TimeCode::from_frames(-100, fps);
    let stack_neg = evaluator.evaluate(&graph_valid, &neg_time);
    assert_eq!(stack_neg.active_count(), 0);
    assert!(stack_neg.render_list.is_empty());

    // B: Way past composition duration (e.g. frame 1,000,000)
    let huge_time = TimeCode::from_frames(1_000_000, fps);
    let stack_huge = evaluator.evaluate(&graph_valid, &huge_time);
    assert_eq!(stack_huge.active_count(), 0);
    assert!(stack_huge.render_list.is_empty());

    // C: Last active frame just before out_point
    let last_active = TimeCode::from_frames(149, fps);
    let stack_last = evaluator.evaluate(&graph_valid, &last_active);
    assert_eq!(stack_last.active_count(), 1);
    assert!(stack_last.get_layer("s").unwrap().is_active);

    // D: Exact out_point (half-open [in, out) makes frame 150 inactive)
    let exact_out = TimeCode::from_frames(150, fps);
    let stack_out = evaluator.evaluate(&graph_valid, &exact_out);
    assert_eq!(stack_out.active_count(), 0);
    assert!(stack_out.render_list.is_empty());
}

// 8. Performance & Throughput Profiling Test:
// High-throughput evaluation test evaluating complex multi-layer compositions across thousands
// of timeline frames (10,000+ frames evaluated).
// Verifies that evaluation throughput easily satisfies real-time timeline scrubbing requirements
// (> 10,000 evaluations/sec in release/test, measuring average evaluation latency per frame).
#[test]
fn test_performance_high_throughput_composition_evaluation() {
    let mut project = Project::with_defaults("Throughput Benchmark Project");
    let fps = 60.0;
    let tc0 = TimeCode::from_frames(0, fps);
    let tc600 = TimeCode::from_frames(600, fps); // 10.0 seconds

    // Nested PreComp (5 layers)
    let mut child_comp = Composition::new("child_comp", "Child Graphics", 1920, 1080, fps, tc600);
    for i in 0..5 {
        let mut layer = project::Layer::solid(
            format!("child_layer_{i}"),
            format!("Child Layer {i}"),
            Color::WHITE,
            200,
            200,
            tc0,
            tc600,
        );
        layer.transform.position.add_keyframe(Keyframe::linear(tc0, Vec2::new(100.0 * i as f32, 50.0)));
        layer.transform.position.add_keyframe(Keyframe::linear(tc600, Vec2::new(300.0 * i as f32, 250.0)));
        child_comp.add_layer(layer).unwrap();
    }

    // Main Root Comp (12 layers total: shapes, solids, text, parenting chain, track matte, and nested comp)
    let mut root_comp = Composition::new("root_comp", "Root Benchmark Comp", 1920, 1080, fps, tc600);

    // Layer 0: Background solid
    let bg = project::Layer::solid("bg", "Background", Color::BLACK, 1920, 1080, tc0, tc600);
    root_comp.add_layer(bg).unwrap();

    // Layers 1..4: Parenting chain with Bezier animated transforms
    for i in 1..=4 {
        let mut node = project::Layer::solid(
            format!("hier_node_{i}"),
            format!("Hier Node {i}"),
            Color::RED,
            100,
            100,
            tc0,
            tc600,
        );
        if i > 1 {
            node.set_parent(Some(format!("hier_node_{}", i - 1)));
        }
        // Bezier position
        node.transform.position.add_keyframe(Keyframe::bezier(
            tc0,
            Vec2::new(50.0, 50.0),
            None,
            Some(KeyframeTangent::ease_in_out_out()),
        ));
        node.transform.position.add_keyframe(Keyframe::bezier(
            tc600,
            Vec2::new(400.0, 200.0),
            Some(KeyframeTangent::ease_in_out_in()),
            None,
        ));
        // Bezier rotation
        node.transform.rotation.add_keyframe(Keyframe::linear(tc0, 0.0));
        node.transform.rotation.add_keyframe(Keyframe::linear(tc600, 360.0));
        root_comp.add_layer(node).unwrap();
    }

    // Layer 5: Track matte mask (shape)
    let matte_src = project::Layer::shape(
        "matte_shape",
        "Matte Shape",
        ShapeType::Ellipse {
            radius_x: Property::new("RX", 150.0),
            radius_y: Property::new("RY", 150.0),
            fill: Color::WHITE,
        fill_gradient: None,
        },
        tc0,
        tc600,
    );
    root_comp.add_layer(matte_src).unwrap();

    // Layer 6: Masked text layer (uses matte_shape)
    let mut masked_text = project::Layer::text(
        "masked_text",
        "Masked Text",
        "Scrubbing Benchmark",
        "Inter",
        64.0,
        Color::WHITE,
        tc0,
        tc600,
    );
    masked_text.set_matte(TrackMatteMode::Alpha, Some("matte_shape"));
    root_comp.add_layer(masked_text).unwrap();

    // Layer 7: Animated opacity fade solid
    let mut fader = project::Layer::solid("fader", "Fader", Color::BLUE, 500, 500, tc0, tc600);
    fader.opacity.add_keyframe(Keyframe::linear(tc0, 0.0));
    fader.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(300, fps), 100.0));
    fader.opacity.add_keyframe(Keyframe::linear(tc600, 0.0));
    root_comp.add_layer(fader).unwrap();

    // Layer 8: Nested composition
    let nested_layer = project::Layer::nested_composition("nested_graphics", "Graphics Precomp", "child_comp", tc0, tc600)
        .with_time_stretch(0.5);
    root_comp.add_layer(nested_layer).unwrap();

    project.add_composition(root_comp).unwrap();
    project.add_composition(child_comp).unwrap();

    let evaluator = LayerStackEvaluator::new();
    let root_graph = SceneGraph::from_project(&project, "root_comp").expect("Build scene graph");

    // Perform high-throughput evaluation across 10,000 frames
    const TOTAL_EVALS: usize = 10_000;
    let start_time = Instant::now();

    for frame in 0..TOTAL_EVALS {
        let t = TimeCode::from_frames((frame % 600) as i64, fps);
        let stack = evaluator
            .evaluate_with_project(&root_graph, &project, &t)
            .expect("Evaluate frame");

        // Verify active layers evaluated properly without breaking invariant
        debug_assert!(stack.active_count() > 0);

        // Also resolve flattened render passes as required for timeline scrubbing rasterization
        let flattened = stack.flattened_render_list();
        debug_assert!(!flattened.is_empty());
    }

    let elapsed = start_time.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let evals_per_sec = TOTAL_EVALS as f64 / elapsed_secs;
    let avg_latency_us = (elapsed_secs * 1_000_000.0) / TOTAL_EVALS as f64;

    println!(
        "\n=======================================================\n\
         Composition Evaluation Performance Profile:\n\
         - Total frames evaluated: {}\n\
         - Total elapsed time:     {:.3} ms\n\
         - Evaluation throughput:  {:.2} evaluations/sec\n\
         - Average frame latency:  {:.2} µs / frame\n\
         =======================================================",
        TOTAL_EVALS,
        elapsed.as_secs_f64() * 1000.0,
        evals_per_sec,
        avg_latency_us
    );

    // Assert that evaluation throughput easily satisfies real-time timeline scrubbing requirements (> 10,000 evals/sec)
    assert!(
        evals_per_sec > 10_000.0,
        "Evaluation throughput too low: {:.2} evals/sec (expected > 10,000 evals/sec)",
        evals_per_sec
    );
}
