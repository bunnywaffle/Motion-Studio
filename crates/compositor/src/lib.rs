pub mod error;
pub mod evaluation;
pub mod fx;
pub mod graph;
pub mod node;
pub mod transform;

pub use error::SceneGraphError;
pub use evaluation::{
    resolve_nested_time, EvaluatedEffect, EvaluatedEffectType, EvaluatedLayer, EvaluatedStack,
    FlattenedRenderLayer, LayerStackEvaluator, NestedCompositionEvaluation, RenderPassDescriptor,
};
pub use graph::SceneGraph;
pub use node::SceneNode;
pub use transform::{AffineTransform2D, BoundingBox2D, EvaluatedTransform, TransformResolver};

#[cfg(test)]
mod tests {
    use super::*;
    use project::{Color, Composition, Layer, Project, TimeCode, Vec2};
    use std::collections::HashMap;

    #[test]
    fn test_empty_composition_scene_graph() {
        let comp = Composition::hd_1080p_30fps("comp_empty", "Empty Composition", 5.0);
        let graph = SceneGraph::from_composition(&comp).expect("Valid graph");

        assert_eq!(graph.composition_id, "comp_empty");
        assert_eq!(graph.node_count(), 0);
        assert!(graph.is_empty());
        assert!(graph.root_nodes().is_empty());
        assert!(graph.evaluation_order().unwrap().is_empty());
        assert!(graph.composite_order().is_empty());
    }

    #[test]
    fn test_multi_layer_scene_graph_construction() {
        let mut comp = Composition::hd_1080p_30fps("comp_multi", "Multi Layer", 10.0);
        let tc0 = TimeCode::zero(30.0);
        let tc300 = TimeCode::from_frames(300, 30.0);

        let l1 = Layer::solid("l1", "Background Solid", Color::BLACK, 1920, 1080, tc0, tc300);
        let l2 = Layer::text("l2", "Title Text", "Hello World", "Inter", 64.0, Color::WHITE, tc0, tc300);
        let l3 = Layer::solid("l3", "Accent Solid", Color::RED, 200, 200, tc0, tc300);

        comp.add_layer(l1).unwrap();
        comp.add_layer(l2).unwrap();
        comp.add_layer(l3).unwrap();

        let graph = SceneGraph::from_composition(&comp).expect("Build scene graph");
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.root_nodes().len(), 3);

        let n1 = graph.get_node("l1").expect("l1 exists");
        assert_eq!(n1.name, "Background Solid");
        assert_eq!(n1.layer_index, 0);
        assert!(n1.is_root());
        assert!(n1.is_leaf());

        let n2 = graph.get_node("l2").expect("l2 exists");
        assert_eq!(n2.layer_index, 1);

        let n3 = graph.get_node("l3").expect("l3 exists");
        assert_eq!(n3.layer_index, 2);
    }

    #[test]
    fn test_topological_evaluation_order_guarantee() {
        // Build a hierarchy where layer stack order is:
        // [0: Child2 (parent: Child1)]
        // [1: Root]
        // [2: Child1 (parent: Root)]
        //
        // Evaluation order MUST be: Root -> Child1 -> Child2, regardless of stack order!
        let mut comp = Composition::hd_1080p_30fps("comp_topo", "Topological Test", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        let mut child2 = Layer::solid("child2", "Child 2", Color::RED, 100, 100, tc0, tc150);
        child2.set_parent(Some("child1"));

        let root = Layer::solid("root", "Root Node", Color::BLACK, 1920, 1080, tc0, tc150);

        let mut child1 = Layer::solid("child1", "Child 1", Color::GREEN, 200, 200, tc0, tc150);
        child1.set_parent(Some("root"));

        comp.add_layer(child2).unwrap();
        comp.add_layer(root).unwrap();
        comp.add_layer(child1).unwrap();

        let graph = SceneGraph::from_composition(&comp).expect("Build scene graph");

        // Stacking order
        let stack: Vec<&str> = graph.layer_stack_order().into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(stack, vec!["child2", "root", "child1"]);

        // Evaluation order
        let eval = graph.evaluation_order().expect("Topological sort succeeds");
        let eval_ids: Vec<&str> = eval.into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(eval_ids, vec!["root", "child1", "child2"]);

        // Verify root node
        assert_eq!(graph.root_nodes().len(), 1);
        assert_eq!(graph.root_nodes()[0].id, "root");

        // Verify hierarchy queries
        assert_eq!(graph.get_parent("child2").unwrap().id, "child1");
        assert_eq!(graph.get_parent("child1").unwrap().id, "root");
        assert!(graph.get_parent("root").is_none());

        assert_eq!(graph.get_children("root").len(), 1);
        assert_eq!(graph.get_children("root")[0].id, "child1");
        assert_eq!(graph.get_children("child1")[0].id, "child2");
        assert!(graph.get_children("child2").is_empty());

        // Ancestor chain for child2: child1, root
        let ancestors = graph.get_ancestor_chain("child2");
        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0].id, "child1");
        assert_eq!(ancestors[1].id, "root");

        // Descendants for root: child1, child2
        let descendants = graph.get_descendants("root");
        assert_eq!(descendants.len(), 2);
        assert_eq!(descendants[0].id, "child1");
        assert_eq!(descendants[1].id, "child2");

        // Depths
        assert_eq!(graph.depth_of("root"), 0);
        assert_eq!(graph.depth_of("child1"), 1);
        assert_eq!(graph.depth_of("child2"), 2);
    }

    #[test]
    fn test_branching_hierarchy_scene_graph() {
        // Root
        //  ├── Branch A
        //  │    ├── Leaf A1
        //  │    └── Leaf A2
        //  └── Branch B
        //       └── Leaf B1
        let mut comp = Composition::hd_1080p_30fps("comp_tree", "Tree Test", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        let root = Layer::solid("root", "Root", Color::BLACK, 1920, 1080, tc0, tc150);

        let mut branch_a = Layer::solid("branch_a", "Branch A", Color::WHITE, 100, 100, tc0, tc150);
        branch_a.set_parent(Some("root"));

        let mut leaf_a1 = Layer::solid("leaf_a1", "Leaf A1", Color::RED, 50, 50, tc0, tc150);
        leaf_a1.set_parent(Some("branch_a"));

        let mut leaf_a2 = Layer::solid("leaf_a2", "Leaf A2", Color::GREEN, 50, 50, tc0, tc150);
        leaf_a2.set_parent(Some("branch_a"));

        let mut branch_b = Layer::solid("branch_b", "Branch B", Color::BLUE, 100, 100, tc0, tc150);
        branch_b.set_parent(Some("root"));

        let mut leaf_b1 = Layer::solid("leaf_b1", "Leaf B1", Color::WHITE, 50, 50, tc0, tc150);
        leaf_b1.set_parent(Some("branch_b"));

        comp.add_layer(root).unwrap();
        comp.add_layer(branch_a).unwrap();
        comp.add_layer(leaf_a1).unwrap();
        comp.add_layer(leaf_a2).unwrap();
        comp.add_layer(branch_b).unwrap();
        comp.add_layer(leaf_b1).unwrap();

        let graph = SceneGraph::from_composition(&comp).expect("Build tree graph");

        let eval = graph.evaluation_order().unwrap();
        let eval_indices: HashMap<&str, usize> = eval
            .iter()
            .enumerate()
            .map(|(idx, n)| (n.id.as_str(), idx))
            .collect();

        // Check topological invariants:
        // Root must come before Branch A and Branch B
        assert!(eval_indices["root"] < eval_indices["branch_a"]);
        assert!(eval_indices["root"] < eval_indices["branch_b"]);

        // Branch A must come before Leaf A1 and Leaf A2
        assert!(eval_indices["branch_a"] < eval_indices["leaf_a1"]);
        assert!(eval_indices["branch_a"] < eval_indices["leaf_a2"]);

        // Branch B must come before Leaf B1
        assert!(eval_indices["branch_b"] < eval_indices["leaf_b1"]);

        // Check descendants
        let root_descendants: Vec<&str> = graph
            .get_descendants("root")
            .into_iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(root_descendants.len(), 5);
        assert!(root_descendants.contains(&"branch_a"));
        assert!(root_descendants.contains(&"branch_b"));
        assert!(root_descendants.contains(&"leaf_a1"));
        assert!(root_descendants.contains(&"leaf_a2"));
        assert!(root_descendants.contains(&"leaf_b1"));
    }

    #[test]
    fn test_deep_parenting_chain_topological_sort() {
        let mut comp = Composition::hd_1080p_30fps("comp_deep", "Deep Chain", 10.0);
        const DEPTH: usize = 50;

        let tc0 = TimeCode::zero(30.0);
        let tc300 = TimeCode::from_frames(300, 30.0);

        // Add layers in reverse order: layer_49, layer_48, ..., layer_0
        for i in (0..DEPTH).rev() {
            let mut layer = Layer::solid(
                format!("node_{i}"),
                format!("Node {i}"),
                Color::WHITE,
                100,
                100,
                tc0,
                tc300,
            );
            if i > 0 {
                layer.set_parent(Some(format!("node_{}", i - 1)));
            }
            comp.add_layer(layer).unwrap();
        }

        let graph = SceneGraph::from_composition(&comp).expect("Build deep graph");
        assert_eq!(graph.node_count(), DEPTH);

        let eval = graph.evaluation_order().expect("Topological sort succeeds");
        assert_eq!(eval.len(), DEPTH);

        // Verify that eval order is strictly node_0, node_1, ..., node_49
        for (i, node) in eval.iter().enumerate().take(DEPTH) {
            assert_eq!(node.id, format!("node_{i}"));
        }
    }

    #[test]
    fn test_cycle_detection_error_handling() {
        let tc0 = TimeCode::zero(30.0);
        let tc100 = TimeCode::from_frames(100, 30.0);

        // Case 1: Self parenting
        let mut comp_self = Composition::hd_1080p_30fps("comp_self", "Self", 5.0);
        let mut l_self = Layer::solid("l_self", "Self", Color::RED, 100, 100, tc0, tc100);
        l_self.set_parent(Some("l_self"));
        comp_self.add_layer(l_self).unwrap();

        assert!(matches!(
            SceneGraph::from_composition(&comp_self),
            Err(SceneGraphError::SelfParenting(ref id)) if id == "l_self"
        ));

        // Case 2: Direct cycle A -> B -> A
        let mut comp_direct = Composition::hd_1080p_30fps("comp_direct", "Direct Cycle", 5.0);
        let mut la = Layer::solid("la", "A", Color::RED, 100, 100, tc0, tc100);
        let mut lb = Layer::solid("lb", "B", Color::RED, 100, 100, tc0, tc100);
        la.set_parent(Some("lb"));
        lb.set_parent(Some("la"));
        comp_direct.add_layer(la).unwrap();
        comp_direct.add_layer(lb).unwrap();

        assert!(matches!(
            SceneGraph::from_composition(&comp_direct),
            Err(SceneGraphError::ParentCycleDetected { .. })
        ));

        // Case 3: Missing parent
        let mut comp_missing = Composition::hd_1080p_30fps("comp_missing", "Missing Parent", 5.0);
        let mut l_orphan = Layer::solid("l_orphan", "Orphan", Color::RED, 100, 100, tc0, tc100);
        l_orphan.set_parent(Some("non_existent_parent"));
        comp_missing.add_layer(l_orphan).unwrap();

        assert!(matches!(
            SceneGraph::from_composition(&comp_missing),
            Err(SceneGraphError::ParentNotFound { .. })
        ));
    }

    #[test]
    fn test_layer_stack_vs_composite_order() {
        let mut comp = Composition::hd_1080p_30fps("comp_render", "Render Order", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // In AE timeline:
        // Index 0: Top Layer
        // Index 1: Middle Layer
        // Index 2: Bottom Layer
        let top = Layer::solid("top", "Top Layer", Color::RED, 100, 100, tc0, tc150);
        let mid = Layer::solid("mid", "Middle Layer", Color::GREEN, 100, 100, tc0, tc150);
        let btm = Layer::solid("btm", "Bottom Layer", Color::BLUE, 100, 100, tc0, tc150);

        comp.add_layer(top).unwrap();
        comp.add_layer(mid).unwrap();
        comp.add_layer(btm).unwrap();

        let graph = SceneGraph::from_composition(&comp).expect("Build graph");

        // Layer stack order: top -> mid -> btm
        let stack: Vec<&str> = graph.layer_stack_order().into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(stack, vec!["top", "mid", "btm"]);

        // Painter's composite order: btm -> mid -> top (bottom renders first!)
        let comp_order: Vec<&str> = graph.composite_order().into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(comp_order, vec!["btm", "mid", "top"]);
    }

    #[test]
    fn test_timecode_activity_and_visibility_filtering() {
        let mut comp = Composition::hd_1080p_30fps("comp_timed", "Timing Filter", 10.0);

        // Layer 1: frames 0..60, visible
        let l1 = Layer::solid("l1", "L1", Color::RED, 100, 100, TimeCode::from_frames(0, 30.0), TimeCode::from_frames(60, 30.0));

        // Layer 2: frames 30..90, visible
        let l2 = Layer::solid("l2", "L2", Color::GREEN, 100, 100, TimeCode::from_frames(30, 30.0), TimeCode::from_frames(90, 30.0));

        // Layer 3: frames 30..90, HIDDEN (visible = false)
        let mut l3 = Layer::solid("l3", "L3", Color::BLUE, 100, 100, TimeCode::from_frames(30, 30.0), TimeCode::from_frames(90, 30.0));
        l3.visible = false;

        comp.add_layer(l1).unwrap();
        comp.add_layer(l2).unwrap();
        comp.add_layer(l3).unwrap();

        let graph = SceneGraph::from_composition(&comp).expect("Build graph");

        // At frame 15: only l1 is active and visible
        let t15 = TimeCode::from_frames(15, 30.0);
        let active_15 = graph.active_nodes_at(&t15);
        assert_eq!(active_15.len(), 1);
        assert_eq!(active_15[0].id, "l1");
        let render_15 = graph.render_order_at(&t15);
        assert_eq!(render_15.len(), 1);
        assert_eq!(render_15[0].id, "l1");

        // At frame 45: l1, l2, l3 are active; but only l1, l2 are visible
        let t45 = TimeCode::from_frames(45, 30.0);
        let active_45: Vec<&str> = graph.active_nodes_at(&t45).into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(active_45, vec!["l1", "l2", "l3"]);

        // Render order (composite order) at frame 45: l2 -> l1 (since l3 is hidden, and composite order is bottom-up)
        let render_45: Vec<&str> = graph.render_order_at(&t45).into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(render_45, vec!["l2", "l1"]);

        // At frame 75: l2 and l3 active; only l2 visible
        let t75 = TimeCode::from_frames(75, 30.0);
        let render_75: Vec<&str> = graph.render_order_at(&t75).into_iter().map(|n| n.id.as_str()).collect();
        assert_eq!(render_75, vec!["l2"]);

        // At frame 120: nothing active
        let t120 = TimeCode::from_frames(120, 30.0);
        assert!(graph.active_nodes_at(&t120).is_empty());
        assert!(graph.render_order_at(&t120).is_empty());
    }

    #[test]
    fn test_from_project_and_error_handling() {
        let mut project = Project::new("proj_test", "Project Test");
        let comp = Composition::hd_1080p_30fps("c1", "Comp 1", 5.0);
        project.add_composition(comp).unwrap();

        let graph = SceneGraph::from_project(&project, "c1").expect("Build graph from project");
        assert_eq!(graph.composition_id, "c1");

        let err = SceneGraph::from_project(&project, "missing_comp");
        assert!(matches!(err, Err(SceneGraphError::CompositionNotFound(ref id)) if id == "missing_comp"));
    }

    #[test]
    fn test_evaluator_exact_boundary_conditions() {
        let mut comp = Composition::hd_1080p_30fps("comp_bound", "Boundary Test", 10.0);
        // Layer spanning exactly frame 30 to frame 60 (half-open [30, 60))
        let layer = Layer::solid(
            "l_span",
            "Span Layer",
            Color::RED,
            100,
            100,
            TimeCode::from_frames(30, 30.0),
            TimeCode::from_frames(60, 30.0),
        );
        comp.add_layer(layer).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();

        // Frame 29 (just before in_point): inactive
        let stack_29 = evaluator.evaluate(&graph, &TimeCode::from_frames(29, 30.0));
        let l_29 = stack_29.get_layer("l_span").unwrap();
        assert!(!l_29.is_active);
        assert!(!l_29.is_visible);
        assert_eq!(l_29.effective_opacity, 0.0);
        assert!(stack_29.render_list.is_empty());

        // Frame 30 (exact in_point): active!
        let stack_30 = evaluator.evaluate(&graph, &TimeCode::from_frames(30, 30.0));
        let l_30 = stack_30.get_layer("l_span").unwrap();
        assert!(l_30.is_active);
        assert!(l_30.is_visible);
        assert_eq!(l_30.effective_opacity, 1.0);
        assert_eq!(l_30.time_offset_frames, 0);
        assert_eq!(l_30.time_offset_seconds, 0.0);
        assert_eq!(stack_30.render_list, vec!["l_span"]);

        // Frame 59 (last active frame before out_point): active!
        let stack_59 = evaluator.evaluate(&graph, &TimeCode::from_frames(59, 30.0));
        let l_59 = stack_59.get_layer("l_span").unwrap();
        assert!(l_59.is_active);
        assert_eq!(l_59.time_offset_frames, 29);
        assert_eq!(stack_59.render_list, vec!["l_span"]);

        // Frame 60 (exact out_point): inactive! (half-open [in, out) convention)
        let stack_60 = evaluator.evaluate(&graph, &TimeCode::from_frames(60, 30.0));
        let l_60 = stack_60.get_layer("l_span").unwrap();
        assert!(!l_60.is_active);
        assert!(!l_60.is_visible);
        assert_eq!(l_60.effective_opacity, 0.0);
        assert!(stack_60.render_list.is_empty());
    }

    #[test]
    fn test_evaluator_hidden_and_opacity_clamping() {
        let mut comp = Composition::hd_1080p_30fps("comp_opacity", "Opacity Test", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        let mut l_50 = Layer::solid("l_50", "50% Opacity", Color::RED, 100, 100, tc0, tc150);
        l_50.opacity.set_value(50.0);

        let mut l_hidden = Layer::solid("l_hidden", "Hidden", Color::BLUE, 100, 100, tc0, tc150);
        l_hidden.visible = false;

        let mut l_over = Layer::solid("l_over", "Over 100%", Color::GREEN, 100, 100, tc0, tc150);
        l_over.opacity.set_value(150.0);

        comp.add_layer(l_50).unwrap();
        comp.add_layer(l_hidden).unwrap();
        comp.add_layer(l_over).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(10, 30.0));

        let eval_50 = stack.get_layer("l_50").unwrap();
        assert_eq!(eval_50.local_opacity, 50.0);
        assert!((eval_50.effective_opacity - 0.5).abs() < 1e-6);

        let eval_hidden = stack.get_layer("l_hidden").unwrap();
        assert!(!eval_hidden.is_visible);
        assert_eq!(eval_hidden.effective_opacity, 0.0);

        let eval_over = stack.get_layer("l_over").unwrap();
        assert_eq!(eval_over.local_opacity, 100.0); // clamped to 100
        assert_eq!(eval_over.effective_opacity, 1.0);

        // Hidden layer excluded from render_list
        assert!(!stack.render_list.contains(&"l_hidden".to_string()));
        assert_eq!(stack.render_count(), 2);
    }

    #[test]
    fn test_evaluator_solo_modes() {
        let mut comp = Composition::hd_1080p_30fps("comp_solo", "Solo Test", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        let l1 = Layer::solid("l1", "Layer 1", Color::RED, 100, 100, tc0, tc150);
        let mut l2 = Layer::solid("l2", "Layer 2", Color::GREEN, 100, 100, tc0, tc150);
        let l3 = Layer::solid("l3", "Layer 3", Color::BLUE, 100, 100, tc0, tc150);

        // Solo Layer 2
        l2.set_solo(true);

        comp.add_layer(l1).unwrap();
        comp.add_layer(l2).unwrap();
        comp.add_layer(l3).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(10, 30.0));

        // When l2 is soloed, l1 and l3 are suppressed
        assert!(!stack.get_layer("l1").unwrap().is_visible);
        assert!(stack.get_layer("l2").unwrap().is_visible);
        assert!(!stack.get_layer("l3").unwrap().is_visible);

        assert_eq!(stack.render_list, vec!["l2"]);
    }

    #[test]
    fn test_evaluator_track_matte_pairing_and_consumption() {
        use project::TrackMatteMode;

        let mut comp = Composition::hd_1080p_30fps("comp_matte", "Matte Test", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Visual stack in timeline:
        // [Index 0] Matte Source (e.g. circle shape)
        // [Index 1] Video/Texture Layer (masked by Matte Source)
        // [Index 2] Background Layer
        let matte_layer = Layer::solid("matte_src", "Matte Shape", Color::WHITE, 200, 200, tc0, tc150);
        let mut masked_layer = Layer::solid("masked_clip", "Video Texture", Color::RED, 1920, 1080, tc0, tc150);
        // Adjacent track matte: masked_clip uses matte_src (the layer above it)
        masked_layer.set_matte(TrackMatteMode::Alpha, None::<String>);

        let bg_layer = Layer::solid("bg", "Background", Color::BLACK, 1920, 1080, tc0, tc150);

        comp.add_layer(matte_layer).unwrap();
        comp.add_layer(masked_layer).unwrap();
        comp.add_layer(bg_layer).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(10, 30.0));

        let eval_matte = stack.get_layer("matte_src").unwrap();
        let eval_masked = stack.get_layer("masked_clip").unwrap();

        // Matte relationship resolved
        assert_eq!(eval_masked.matte_mode, TrackMatteMode::Alpha);
        assert_eq!(eval_masked.matte_source_id.as_deref(), Some("matte_src"));
        assert!(eval_matte.is_matte_source);

        // Painter's composite render list:
        // bg (index 2) -> masked_clip (index 1)
        // Note: matte_src (index 0) is consumed as a mask, so it is NOT rendered directly!
        assert_eq!(stack.render_list, vec!["bg", "masked_clip"]);

        // When consume_matte_sources = false, matte_src is also included in direct render
        let mut non_consuming_evaluator = LayerStackEvaluator::new();
        non_consuming_evaluator.consume_matte_sources = false;
        let stack_all = non_consuming_evaluator.evaluate(&graph, &TimeCode::from_frames(10, 30.0));
        assert_eq!(stack_all.render_list, vec!["bg", "masked_clip", "matte_src"]);
    }

    #[test]
    fn test_evaluator_explicit_track_matte_targeting() {
        use project::TrackMatteMode;

        let mut comp = Composition::hd_1080p_30fps("comp_exp_matte", "Explicit Matte", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Target matte layer located elsewhere in the stack (non-adjacent)
        let l1 = Layer::solid("l1", "Layer 1", Color::RED, 100, 100, tc0, tc150);
        let l2 = Layer::solid("l2", "Layer 2", Color::GREEN, 100, 100, tc0, tc150);
        let mut l3 = Layer::solid("l3", "Layer 3", Color::BLUE, 100, 100, tc0, tc150);

        // l3 explicitly targets l1 as its Luma matte
        l3.set_matte(TrackMatteMode::Luma, Some("l1"));

        comp.add_layer(l1).unwrap();
        comp.add_layer(l2).unwrap();
        comp.add_layer(l3).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(10, 30.0));

        let eval_l3 = stack.get_layer("l3").unwrap();
        assert_eq!(eval_l3.matte_mode, TrackMatteMode::Luma);
        assert_eq!(eval_l3.matte_source_id.as_deref(), Some("l1"));

        let eval_l1 = stack.get_layer("l1").unwrap();
        assert!(eval_l1.is_matte_source);
    }

    #[test]
    fn test_identity_transform_and_defaults() {
        use project::Vec2;

        let id = AffineTransform2D::IDENTITY;
        assert_eq!(id.a, 1.0);
        assert_eq!(id.b, 0.0);
        assert_eq!(id.c, 0.0);
        assert_eq!(id.d, 1.0);
        assert_eq!(id.tx, 0.0);
        assert_eq!(id.ty, 0.0);
        assert_eq!(id.determinant(), 1.0);
        assert!(id.is_invertible());

        let p = Vec2::new(142.5, -87.25);
        assert_eq!(id.transform_point(p), p);
        assert_eq!(id * p, p);
        assert_eq!(id.transform_vector(p), p);

        let inv = id.inverse().expect("Identity is invertible");
        assert_eq!(inv, id);

        let eval_def = EvaluatedTransform::default();
        assert_eq!(eval_def.local_matrix, AffineTransform2D::IDENTITY);
        assert_eq!(eval_def.world_matrix, AffineTransform2D::IDENTITY);
        assert_eq!(eval_def.anchor_point, Vec2::ZERO);
        assert_eq!(eval_def.position, Vec2::ZERO);
        assert_eq!(eval_def.scale, Vec2::SCALE_100);
        assert_eq!(eval_def.rotation, 0.0);
        assert!(eval_def.is_invertible());
    }

    #[test]
    fn test_individual_transform_components() {
        use project::Vec2;

        // 1. Translation
        let t = AffineTransform2D::from_translation(Vec2::new(250.0, -150.0));
        assert_eq!(t.transform_point(Vec2::ZERO), Vec2::new(250.0, -150.0));
        assert_eq!(t.transform_point(Vec2::new(10.0, 20.0)), Vec2::new(260.0, -130.0));
        // Vector transformation ignores translation
        assert_eq!(t.transform_vector(Vec2::new(10.0, 20.0)), Vec2::new(10.0, 20.0));

        // 2. Scale
        let s = AffineTransform2D::from_scale(Vec2::new(2.5, 0.5));
        assert_eq!(s.transform_point(Vec2::new(100.0, 100.0)), Vec2::new(250.0, 50.0));
        assert_eq!(s.determinant(), 1.25);

        // 3. Rotation (90, 180, 270, 360 degrees)
        let r90 = AffineTransform2D::from_rotation_degrees(90.0);
        let p_unit_x = Vec2::new(1.0, 0.0);
        let p_rot90 = r90.transform_point(p_unit_x);
        assert!((p_rot90.x - 0.0).abs() < 1e-5);
        assert!((p_rot90.y - 1.0).abs() < 1e-5); // Clockwise in screen coordinates maps (1,0) -> (0,1)

        let r180 = AffineTransform2D::from_rotation_degrees(180.0);
        let p_rot180 = r180.transform_point(p_unit_x);
        assert!((p_rot180.x - (-1.0)).abs() < 1e-5);
        assert!((p_rot180.y - 0.0).abs() < 1e-5);

        let r360 = AffineTransform2D::from_rotation_degrees(360.0);
        assert!(r360.approx_eq(&AffineTransform2D::IDENTITY, 1e-5));

        // 4. Anchor Point offsetting:
        // When layer is at position (300, 200) with anchor point (50, 50),
        // local anchor point (50, 50) MUST map to world position (300, 200).
        let comp_trans = AffineTransform2D::from_transform_components(
            Vec2::new(300.0, 200.0),
            Vec2::SCALE_100,
            0.0,
            Vec2::new(50.0, 50.0),
        );
        assert_eq!(comp_trans.transform_point(Vec2::new(50.0, 50.0)), Vec2::new(300.0, 200.0));
        // Local top-left (0, 0) should be at (300 - 50, 200 - 50) = (250, 150)
        assert_eq!(comp_trans.transform_point(Vec2::ZERO), Vec2::new(250.0, 150.0));
    }

    #[test]
    fn test_combined_local_transform_order() {
        use project::Vec2;

        // Verify order: T(pos) * R(rot) * S(scale) * T(-anchor)
        let pos = Vec2::new(500.0, 500.0);
        let anchor = Vec2::new(100.0, 100.0);
        let scale = Vec2::new(200.0, 200.0); // 2x scale
        let rot = 90.0; // 90 deg clockwise

        let m_local = AffineTransform2D::from_transform_components(pos, scale, rot, anchor);

        // 1. Anchor point MUST map exactly to Position
        let p_anchor_world = m_local.transform_point(anchor);
        assert!((p_anchor_world.x - pos.x).abs() < 1e-4);
        assert!((p_anchor_world.y - pos.y).abs() < 1e-4);

        // 2. Point 50 units to the right of anchor in local space: (150, 100)
        // Offset from anchor = (50, 0)
        // Scaled by 2x = (100, 0)
        // Rotated 90 deg clockwise in screen space = (0, 100)
        // Translated to pos (500, 500) = (500, 600)
        let p_right = Vec2::new(150.0, 100.0);
        let p_right_world = m_local.transform_point(p_right);
        assert!((p_right_world.x - 500.0).abs() < 1e-4);
        assert!((p_right_world.y - 600.0).abs() < 1e-4);
    }

    #[test]
    fn test_matrix_algebra_and_3x3_conversions() {
        use project::Vec2;

        let m1 = AffineTransform2D::from_translation(Vec2::new(10.0, 20.0));
        let m2 = AffineTransform2D::from_scale(Vec2::new(2.0, 3.0));
        let m3 = AffineTransform2D::from_rotation_degrees(45.0);

        // Associativity: (m1 * m2) * m3 == m1 * (m2 * m3)
        let a = (m1 * m2) * m3;
        let b = m1 * (m2 * m3);
        assert!(a.approx_eq(&b, 1e-5));

        // 3x3 matrix conversion roundtrip
        let mat_array = a.to_matrix_3x3();
        let from_mat = AffineTransform2D::from_matrix_3x3(mat_array);
        assert!(a.approx_eq(&from_mat, 1e-5));

        let flat = a.to_matrix_3x3_flat();
        assert_eq!(flat.len(), 9);
        assert_eq!(flat[8], 1.0);
        assert_eq!(flat[7], 0.0);
        assert_eq!(flat[6], 0.0);

        // Determinant of scale and rotation
        let rot_only = AffineTransform2D::from_rotation_degrees(33.0);
        assert!((rot_only.determinant() - 1.0).abs() < 1e-5);

        let scale_only = AffineTransform2D::from_scale(Vec2::new(3.0, 4.0));
        assert!((scale_only.determinant() - 12.0).abs() < 1e-5);
    }

    #[test]
    fn test_bounding_box_mapping_and_operations() {
        use project::Vec2;

        let bbox = BoundingBox2D::from_origin_size(Vec2::new(10.0, 20.0), Vec2::new(100.0, 50.0));
        assert_eq!(bbox.min, Vec2::new(10.0, 20.0));
        assert_eq!(bbox.max, Vec2::new(110.0, 70.0));
        assert_eq!(bbox.width(), 100.0);
        assert_eq!(bbox.height(), 50.0);
        assert_eq!(bbox.center(), Vec2::new(60.0, 45.0));
        assert!(bbox.contains_point(Vec2::new(50.0, 50.0)));
        assert!(!bbox.contains_point(Vec2::new(5.0, 50.0)));

        // Transform bbox by 90 degree rotation around origin and translation (200, 200)
        let transform = AffineTransform2D::from_translation(Vec2::new(200.0, 200.0))
            * AffineTransform2D::from_rotation_degrees(90.0);

        let transformed_bbox = bbox.transform(&transform);
        // Original corners: (10, 20), (110, 20), (110, 70), (10, 70)
        // Rotated 90 deg clockwise (x, y) -> (-y, x):
        // (-20, 10), (-20, 110), (-70, 110), (-70, 10)
        // Translated by (200, 200):
        // (180, 210), (180, 310), (130, 310), (130, 210)
        // Resulting AABB: min = (130, 210), max = (180, 310)
        assert!((transformed_bbox.min.x - 130.0).abs() < 1e-4);
        assert!((transformed_bbox.min.y - 210.0).abs() < 1e-4);
        assert!((transformed_bbox.max.x - 180.0).abs() < 1e-4);
        assert!((transformed_bbox.max.y - 310.0).abs() < 1e-4);
        assert!((transformed_bbox.width() - 50.0).abs() < 1e-4);
        assert!((transformed_bbox.height() - 100.0).abs() < 1e-4);

        // Inverse transform roundtrip
        let recovered_bbox = transformed_bbox.transform_inverse(&transform).unwrap();
        assert!((recovered_bbox.min.x - bbox.min.x).abs() < 1e-4);
        assert!((recovered_bbox.min.y - bbox.min.y).abs() < 1e-4);
        assert!((recovered_bbox.max.x - bbox.max.x).abs() < 1e-4);
        assert!((recovered_bbox.max.y - bbox.max.y).abs() < 1e-4);
    }

    #[test]
    fn test_parent_child_matrix_concatenation() {
        use project::Vec2;

        let mut comp = Composition::hd_1080p_30fps("comp_parenting", "Parenting Test", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Parent Layer: placed at (1000, 500), rotated 90 degrees clockwise, anchor (0, 0)
        let mut parent = Layer::solid("parent", "Parent Layer", Color::RED, 200, 200, tc0, tc150);
        parent.transform.position.set_value(Vec2::new(1000.0, 500.0));
        parent.transform.rotation.set_value(90.0);
        parent.transform.anchor_point.set_value(Vec2::ZERO);

        // Child Layer: parented to "parent", local position (200, 0), rotation 0, anchor (0, 0)
        let mut child = Layer::solid("child", "Child Layer", Color::BLUE, 100, 100, tc0, tc150);
        child.set_parent(Some("parent"));
        child.transform.position.set_value(Vec2::new(200.0, 0.0));
        child.transform.rotation.set_value(0.0);
        child.transform.anchor_point.set_value(Vec2::ZERO);

        comp.add_layer(parent).unwrap();
        comp.add_layer(child).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let transforms = graph.evaluate_transforms().expect("Transforms resolve");

        let parent_t = transforms.get("parent").unwrap();
        let child_t = transforms.get("child").unwrap();

        // Parent local origin (0, 0) maps to (1000, 500)
        let p_world = parent_t.local_to_world_point(Vec2::ZERO);
        assert_eq!(p_world, Vec2::new(1000.0, 500.0));

        // Child local origin (0, 0):
        // In child local space: (0, 0)
        // Position in parent space: (200, 0)
        // In parent space, rotated 90 deg clockwise: (200, 0) -> (0, 200)
        // Translated by parent position (1000, 500): -> (1000, 700)
        let child_origin_world = child_t.local_to_world_point(Vec2::ZERO);
        assert!((child_origin_world.x - 1000.0).abs() < 1e-4);
        assert!((child_origin_world.y - 700.0).abs() < 1e-4);

        // Child point (0, 50) [50 units down in child space]:
        // In parent space, rotated 90 deg clockwise: (0, 50) -> (-50, 0)
        // Relative to child pos (200, 0): (200, 50) rotated -> (-50, 200)
        // Translated by parent pos (1000, 500): -> (950, 700)
        let child_down_world = child_t.local_to_world_point(Vec2::new(0.0, 50.0));
        assert!((child_down_world.x - 950.0).abs() < 1e-4);
        assert!((child_down_world.y - 700.0).abs() < 1e-4);
    }

    #[test]
    fn test_deep_parenting_chains_transform_evaluation() {
        use project::Vec2;

        let mut comp = Composition::hd_1080p_30fps("comp_chain", "Deep Chain Transforms", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);
        const CHAIN_LENGTH: usize = 20;

        // Create a 20-layer chain where each child is offset by (10.0, 5.0) relative to its parent
        for i in 0..CHAIN_LENGTH {
            let mut layer = Layer::solid(
                format!("layer_{i}"),
                format!("Layer {i}"),
                Color::WHITE,
                50,
                50,
                tc0,
                tc150,
            );
            layer.transform.position.set_value(Vec2::new(10.0, 5.0));
            layer.transform.anchor_point.set_value(Vec2::ZERO);

            if i > 0 {
                layer.set_parent(Some(format!("layer_{}", i - 1)));
            }
            comp.add_layer(layer).unwrap();
        }

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let transforms = graph.evaluate_transforms().unwrap();

        // Node 0 world origin: (10.0, 5.0)
        let t0 = transforms.get("layer_0").unwrap();
        assert_eq!(t0.local_to_world_point(Vec2::ZERO), Vec2::new(10.0, 5.0));

        // Node 19 world origin: 20 * (10.0, 5.0) = (200.0, 100.0)
        let t19 = transforms.get("layer_19").unwrap();
        let origin_19 = t19.local_to_world_point(Vec2::ZERO);
        assert!((origin_19.x - 200.0).abs() < 1e-3);
        assert!((origin_19.y - 100.0).abs() < 1e-3);

        // Also test a 4-level chain with 90 degree rotations accumulating to 360 degrees
        let mut rot_comp = Composition::hd_1080p_30fps("comp_rot_chain", "Rot Chain", 5.0);
        for i in 0..4 {
            let mut layer = Layer::solid(
                format!("rot_{i}"),
                format!("Rot {i}"),
                Color::WHITE,
                50,
                50,
                tc0,
                tc150,
            );
            layer.transform.rotation.set_value(90.0);
            layer.transform.position.set_value(Vec2::ZERO);
            layer.transform.anchor_point.set_value(Vec2::ZERO);
            if i > 0 {
                layer.set_parent(Some(format!("rot_{}", i - 1)));
            }
            rot_comp.add_layer(layer).unwrap();
        }

        let rot_graph = SceneGraph::from_composition(&rot_comp).unwrap();
        let rot_transforms = rot_graph.evaluate_transforms().unwrap();

        let t3 = rot_transforms.get("rot_3").unwrap();
        // 4 * 90 = 360 degrees accumulated rotation = Identity!
        assert!(t3.world_matrix.approx_eq(&AffineTransform2D::IDENTITY, 1e-4));
    }

    #[test]
    fn test_invertibility_and_world_to_local_roundtrip() {
        use project::Vec2;

        let mut comp = Composition::hd_1080p_30fps("comp_invert", "Invertibility", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Layer with non-trivial values
        let mut layer = Layer::solid("l_complex", "Complex", Color::GREEN, 200, 200, tc0, tc150);
        layer.transform.position.set_value(Vec2::new(743.2, 381.9));
        layer.transform.anchor_point.set_value(Vec2::new(45.0, 92.5));
        layer.transform.scale.set_value(Vec2::new(140.0, 75.0));
        layer.transform.rotation.set_value(37.5);
        comp.add_layer(layer).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let eval_transform = graph.get_evaluated_transform("l_complex").unwrap();
        assert!(eval_transform.is_invertible());

        // Test multiple arbitrary points for exact roundtrip mapping
        let test_points = [
            Vec2::ZERO,
            Vec2::new(100.0, 100.0),
            Vec2::new(-45.0, -92.5),
            Vec2::new(1920.0, 1080.0),
            Vec2::new(-350.25, 874.125),
        ];

        for local_pt in test_points {
            let world_pt = eval_transform.local_to_world_point(local_pt);
            let recovered_local = eval_transform
                .world_to_local_point(world_pt)
                .expect("World to local point succeeds");

            assert!(
                local_pt.distance_to(recovered_local) < 1e-3,
                "Failed roundtrip for point {:?}: got {:?}",
                local_pt,
                recovered_local
            );
        }

        // Test non-invertible transform (scale x is 0)
        let non_invertible = AffineTransform2D::from_transform_components(
            Vec2::new(100.0, 100.0),
            Vec2::new(0.0, 100.0), // 0% scale along x
            0.0,
            Vec2::ZERO,
        );
        assert!(!non_invertible.is_invertible());
        assert_eq!(non_invertible.determinant(), 0.0);
        assert!(non_invertible.inverse().is_none());
        assert!(non_invertible.transform_point_inverse(Vec2::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn test_pipeline_integration_evaluated_layer_transforms() {
        use project::Vec2;

        let mut comp = Composition::hd_1080p_30fps("comp_pipeline", "Pipeline Transforms", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        let mut parent = Layer::solid("p_layer", "Parent Layer", Color::RED, 400, 300, tc0, tc150);
        parent.transform.position.set_value(Vec2::new(500.0, 400.0));

        let mut child = Layer::solid("c_layer", "Child Layer", Color::BLUE, 100, 100, tc0, tc150);
        child.set_parent(Some("p_layer"));
        child.transform.position.set_value(Vec2::new(50.0, 50.0));

        comp.add_layer(parent).unwrap();
        comp.add_layer(child).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(10, 30.0));

        let parent_eval = stack.get_layer("p_layer").unwrap();
        let child_eval = stack.get_layer("c_layer").unwrap();

        // Local matrices
        assert_eq!(parent_eval.local_matrix().tx, 500.0);
        assert_eq!(parent_eval.local_matrix().ty, 400.0);

        // World matrices
        assert_eq!(child_eval.world_matrix().tx, 550.0);
        assert_eq!(child_eval.world_matrix().ty, 450.0);

        // Layer convenience point mapping
        let world_pt = child_eval.local_to_world_point(Vec2::new(10.0, 10.0));
        assert_eq!(world_pt, Vec2::new(560.0, 460.0));

        let local_recovered = child_eval.world_to_local_point(world_pt).unwrap();
        assert_eq!(local_recovered, Vec2::new(10.0, 10.0));

        // World bounds calculation for 100x100 child layer
        let bounds = child_eval.world_bounds(100.0, 100.0);
        assert_eq!(bounds.min, Vec2::new(550.0, 450.0));
        assert_eq!(bounds.max, Vec2::new(650.0, 550.0));
        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 100.0);

        // EvaluatedStack transform query
        let t_query = stack.get_transform("c_layer").unwrap();
        assert_eq!(t_query.world_matrix, child_eval.world_matrix());
    }

    #[test]
    fn test_negative_scaling_and_mirroring() {
        use project::Vec2;

        // Negative horizontal scale (mirror along X): scale = (-100%, 100%)
        let m_mirror = AffineTransform2D::from_transform_components(
            Vec2::new(200.0, 100.0),
            Vec2::new(-100.0, 100.0),
            0.0,
            Vec2::ZERO,
        );

        assert_eq!(m_mirror.determinant(), -1.0);
        assert!(m_mirror.is_invertible());

        // Point (50, 20) in mirrored layer:
        // x: 200 - 50 = 150
        // y: 100 + 20 = 120
        let p = m_mirror.transform_point(Vec2::new(50.0, 20.0));
        assert_eq!(p, Vec2::new(150.0, 120.0));

        // Inverse roundtrip
        let p_rec = m_mirror.transform_point_inverse(p).unwrap();
        assert!((p_rec.x - 50.0).abs() < 1e-5);
        assert!((p_rec.y - 20.0).abs() < 1e-5);

        // Bounding box under mirror
        let bbox = BoundingBox2D::from_origin_size(Vec2::ZERO, Vec2::new(100.0, 50.0));
        let transformed_bbox = bbox.transform(&m_mirror);
        // Corners: (0,0)->(200,100), (100,0)->(100,100), (100,50)->(100,150), (0,50)->(200,150)
        // Extrema: min = (100, 100), max = (200, 150)
        assert_eq!(transformed_bbox.min, Vec2::new(100.0, 100.0));
        assert_eq!(transformed_bbox.max, Vec2::new(200.0, 150.0));
        assert_eq!(transformed_bbox.width(), 100.0);
        assert_eq!(transformed_bbox.height(), 50.0);

        // Mirror + rotation + anchor point
        let m_complex_mirror = AffineTransform2D::from_transform_components(
            Vec2::new(400.0, 300.0),
            Vec2::new(-200.0, 150.0),
            45.0,
            Vec2::new(50.0, 25.0),
        );
        assert!(m_complex_mirror.is_invertible());
        let test_pt = Vec2::new(123.4, 56.7);
        let world = m_complex_mirror.transform_point(test_pt);
        let recovered = m_complex_mirror.transform_point_inverse(world).unwrap();
        assert!((recovered.x - test_pt.x).abs() < 1e-3);
        assert!((recovered.y - test_pt.y).abs() < 1e-3);
    }

    #[test]
    fn test_simultaneous_parent_child_full_transforms() {
        use project::Vec2;

        let mut comp = Composition::hd_1080p_30fps("comp_simul", "Simultaneous Transforms", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Parent with all 4 components non-trivial:
        let mut parent = Layer::solid("p", "Parent", Color::RED, 300, 200, tc0, tc150);
        parent.transform.position.set_value(Vec2::new(400.0, 300.0));
        parent.transform.anchor_point.set_value(Vec2::new(50.0, 40.0));
        parent.transform.scale.set_value(Vec2::new(150.0, 80.0));
        parent.transform.rotation.set_value(35.0);

        // Child with all 4 components non-trivial and parented to "p":
        let mut child = Layer::solid("c", "Child", Color::BLUE, 100, 80, tc0, tc150);
        child.set_parent(Some("p"));
        child.transform.position.set_value(Vec2::new(120.0, -45.0));
        child.transform.anchor_point.set_value(Vec2::new(25.0, 20.0));
        child.transform.scale.set_value(Vec2::new(80.0, 120.0));
        child.transform.rotation.set_value(-20.0);

        comp.add_layer(parent).unwrap();
        comp.add_layer(child).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let transforms = graph.evaluate_transforms().unwrap();

        let parent_eval = transforms.get("p").unwrap();
        let child_eval = transforms.get("c").unwrap();

        // Mathematical invariant: for ANY local point P in child,
        // child_eval.world_matrix * P == parent_eval.world_matrix * (child_eval.local_matrix * P)
        let pts = [
            Vec2::ZERO,
            Vec2::new(25.0, 20.0), // child anchor point
            Vec2::new(100.0, 80.0),
            Vec2::new(-30.0, 70.0),
        ];

        for pt in pts {
            let direct_world = child_eval.local_to_world_point(pt);
            let child_in_parent = child_eval.local_matrix.transform_point(pt);
            let step_by_step_world = parent_eval.local_to_world_point(child_in_parent);

            assert!(
                (direct_world.x - step_by_step_world.x).abs() < 1e-4,
                "X mismatch for point {:?}: direct={}, step={}",
                pt, direct_world.x, step_by_step_world.x
            );
            assert!(
                (direct_world.y - step_by_step_world.y).abs() < 1e-4,
                "Y mismatch for point {:?}: direct={}, step={}",
                pt, direct_world.y, step_by_step_world.y
            );

            // Invertibility roundtrip
            let recovered = child_eval.world_to_local_point(direct_world).unwrap();
            assert!((recovered.x - pt.x).abs() < 1e-3);
            assert!((recovered.y - pt.y).abs() < 1e-3);
        }
    }

    #[test]
    fn test_deep_100_level_parenting_chain() {
        use project::Vec2;

        let mut comp = Composition::hd_1080p_30fps("comp_deep100", "Deep 100 Chain", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);
        const CHAIN_LENGTH: usize = 100;

        for i in 0..CHAIN_LENGTH {
            let mut layer = Layer::solid(
                format!("l_{i}"),
                format!("Layer {i}"),
                Color::WHITE,
                50,
                50,
                tc0,
                tc150,
            );
            layer.transform.position.set_value(Vec2::new(2.5, 1.5));
            layer.transform.anchor_point.set_value(Vec2::ZERO);

            if i > 0 {
                layer.set_parent(Some(format!("l_{}", i - 1)));
            }
            comp.add_layer(layer).unwrap();
        }

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let transforms = graph.evaluate_transforms().unwrap();
        assert_eq!(transforms.len(), CHAIN_LENGTH);

        // Node 99 world origin should be 100 * (2.5, 1.5) = (250.0, 150.0)
        let t99 = transforms.get("l_99").unwrap();
        let pt99 = t99.local_to_world_point(Vec2::ZERO);
        assert!((pt99.x - 250.0).abs() < 1e-2);
        assert!((pt99.y - 150.0).abs() < 1e-2);

        // Invertibility roundtrip through 100 concatenated matrices
        let recovered = t99.world_to_local_point(pt99).unwrap();
        assert!(recovered.distance_to(Vec2::ZERO) < 1e-2);
    }

    #[test]
    fn test_bounding_box_extended_operations() {
        use project::Vec2;

        let b1 = BoundingBox2D::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0));
        let b2 = BoundingBox2D::new(Vec2::new(50.0, 50.0), Vec2::new(150.0, 150.0));
        let b3 = BoundingBox2D::new(Vec2::new(200.0, 200.0), Vec2::new(300.0, 300.0));

        assert_eq!(b1.area(), 10000.0);
        assert!(!b1.is_empty());

        let empty = BoundingBox2D::ZERO;
        assert!(empty.is_empty());
        assert_eq!(empty.area(), 0.0);

        // Intersection between b1 and b2
        let inter12 = b1.intersection(&b2).unwrap();
        assert_eq!(inter12.min, Vec2::new(50.0, 50.0));
        assert_eq!(inter12.max, Vec2::new(100.0, 100.0));
        assert_eq!(inter12.width(), 50.0);
        assert_eq!(inter12.height(), 50.0);

        // Disjoint boxes b1 and b3
        assert!(!b1.intersects(&b3));
        assert!(b1.intersection(&b3).is_none());

        // Union
        let union12 = b1.union(&b2);
        assert_eq!(union12.min, Vec2::new(0.0, 0.0));
        assert_eq!(union12.max, Vec2::new(150.0, 150.0));
    }

    #[test]
    fn test_matrix_array_conversions_and_singular_handling() {
        use project::Vec2;

        // 1. Column-major 6-float array roundtrip
        let t = AffineTransform2D::from_transform_components(
            Vec2::new(10.0, 20.0),
            Vec2::new(50.0, 150.0),
            45.0,
            Vec2::new(5.0, 5.0),
        );
        let cols = t.to_cols_array();
        let from_cols = AffineTransform2D::from_cols_array(cols);
        assert_eq!(t, from_cols);

        // 2. Flat 9-float array roundtrip
        let flat = t.to_matrix_3x3_flat();
        let from_flat = AffineTransform2D::from_matrix_3x3_flat(flat);
        assert!(t.approx_eq(&from_flat, 1e-5));

        // 3. Column-major 3x3 array check
        let cols_3x3 = t.to_matrix_3x3_cols();
        assert_eq!(cols_3x3[0][0], t.a);
        assert_eq!(cols_3x3[0][1], t.b);
        assert_eq!(cols_3x3[0][2], 0.0);
        assert_eq!(cols_3x3[1][0], t.c);
        assert_eq!(cols_3x3[1][1], t.d);
        assert_eq!(cols_3x3[1][2], 0.0);
        assert_eq!(cols_3x3[2][0], t.tx);
        assert_eq!(cols_3x3[2][1], t.ty);
        assert_eq!(cols_3x3[2][2], 1.0);

        // 4. Non-finite values handling (NaN / Inf)
        let nan_matrix = AffineTransform2D::new(f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0);
        assert!(!nan_matrix.is_finite());
        assert!(!nan_matrix.is_invertible());
        assert!(nan_matrix.inverse().is_none());

        let inf_matrix = AffineTransform2D::new(1.0, 0.0, 0.0, f32::INFINITY, 0.0, 0.0);
        assert!(!inf_matrix.is_finite());
        assert!(!inf_matrix.is_invertible());
        assert!(inf_matrix.inverse().is_none());
    }

    #[test]
    fn test_animated_layer_opacity_evaluation() {
        use project::Keyframe;

        let fps = 30.0;
        let mut comp = Composition::hd_1080p_30fps("comp_anim_op", "Opacity Animation", 5.0);
        let tc0 = TimeCode::from_frames(0, fps);
        let tc60 = TimeCode::from_frames(60, fps);

        let mut layer = Layer::solid("fade_layer", "Fader", Color::WHITE, 1920, 1080, tc0, tc60);
        // Keyframe opacity: 0% at frame 0 -> 100% at frame 30 (1 second fade-in)
        layer.opacity.add_keyframe(Keyframe::linear(tc0, 0.0f32));
        layer.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(30, fps), 100.0f32));

        comp.add_layer(layer).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();

        // Frame 0: Opacity 0%
        let stack_0 = evaluator.evaluate(&graph, &tc0);
        let l_0 = stack_0.get_layer("fade_layer").unwrap();
        assert_eq!(l_0.local_opacity, 0.0);
        assert_eq!(l_0.effective_opacity, 0.0);
        assert!(!l_0.is_rendered()); // effective opacity 0.0 is not rendered
        assert_eq!(stack_0.render_list, vec!["fade_layer"]);

        // Frame 15: Opacity 50%
        let stack_15 = evaluator.evaluate(&graph, &TimeCode::from_frames(15, fps));
        let l_15 = stack_15.get_layer("fade_layer").unwrap();
        assert!((l_15.local_opacity - 50.0).abs() < 1e-4);
        assert!((l_15.effective_opacity - 0.5).abs() < 1e-4);
        assert!(l_15.is_rendered());
        assert_eq!(stack_15.render_list, vec!["fade_layer"]);

        // Frame 30: Opacity 100%
        let stack_30 = evaluator.evaluate(&graph, &TimeCode::from_frames(30, fps));
        let l_30 = stack_30.get_layer("fade_layer").unwrap();
        assert_eq!(l_30.local_opacity, 100.0);
        assert_eq!(l_30.effective_opacity, 1.0);
        assert!(l_30.is_rendered());

        // Frame 45: Opacity holds at 100% (post-keyframe hold)
        let stack_45 = evaluator.evaluate(&graph, &TimeCode::from_frames(45, fps));
        let l_45 = stack_45.get_layer("fade_layer").unwrap();
        assert_eq!(l_45.local_opacity, 100.0);
        assert_eq!(l_45.effective_opacity, 1.0);
    }

    #[test]
    fn test_animated_layer_transform_with_bezier_easing() {
        use project::{Keyframe, KeyframeTangent, Vec2};

        let fps = 30.0;
        let mut comp = Composition::hd_1080p_30fps("comp_anim_trans", "Transform Animation", 5.0);
        let tc0 = TimeCode::from_frames(0, fps);
        let tc30 = TimeCode::from_frames(30, fps);
        let tc60 = TimeCode::from_frames(60, fps);

        let mut layer = Layer::solid("box", "Moving Box", Color::RED, 100, 100, tc0, tc60);
        layer.transform.anchor_point.set_value(Vec2::ZERO);

        // Position: moves from (100, 100) at frame 0 to (500, 500) at frame 30 with ease-in-out
        layer.transform.position.add_keyframe(Keyframe::bezier(
            tc0,
            Vec2::new(100.0, 100.0),
            None,
            Some(KeyframeTangent::ease_in_out_out()),
        ));
        layer.transform.position.add_keyframe(Keyframe::bezier(
            tc30,
            Vec2::new(500.0, 500.0),
            Some(KeyframeTangent::ease_in_out_in()),
            None,
        ));

        // Rotation: rotates from 0 to 90 degrees linearly over frames 0..30
        layer.transform.rotation.add_keyframe(Keyframe::linear(tc0, 0.0));
        layer.transform.rotation.add_keyframe(Keyframe::linear(tc30, 90.0));

        // Scale: scales from 100% to 200% over frames 0..30
        layer.transform.scale.add_keyframe(Keyframe::linear(tc0, Vec2::SCALE_100));
        layer.transform.scale.add_keyframe(Keyframe::linear(tc30, Vec2::new(200.0, 200.0)));

        comp.add_layer(layer).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();

        // 1. Frame 0: Pos (100, 100), Rot 0, Scale 100%
        let stack_0 = evaluator.evaluate(&graph, &tc0);
        let t_0 = stack_0.get_transform("box").unwrap();
        assert_eq!(t_0.position, Vec2::new(100.0, 100.0));
        assert_eq!(t_0.rotation, 0.0);
        assert_eq!(t_0.scale, Vec2::SCALE_100);
        assert_eq!(t_0.local_to_world_point(Vec2::ZERO), Vec2::new(100.0, 100.0));
        let bounds_0 = t_0.world_bounds(100.0, 100.0);
        assert_eq!(bounds_0.min, Vec2::new(100.0, 100.0));
        assert_eq!(bounds_0.max, Vec2::new(200.0, 200.0));

        // 2. Frame 15 (Midpoint): Pos should be halfway (300, 300) due to symmetric ease-in-out
        let stack_15 = evaluator.evaluate(&graph, &TimeCode::from_frames(15, fps));
        let t_15 = stack_15.get_transform("box").unwrap();
        assert!((t_15.position.x - 300.0).abs() < 1.0);
        assert!((t_15.position.y - 300.0).abs() < 1.0);
        assert!((t_15.rotation - 45.0).abs() < 1e-4);
        assert!((t_15.scale.x - 150.0).abs() < 1e-4);

        // 3. Frame 30: Pos (500, 500), Rot 90, Scale 200%
        let stack_30 = evaluator.evaluate(&graph, &tc30);
        let t_30 = stack_30.get_transform("box").unwrap();
        assert_eq!(t_30.position, Vec2::new(500.0, 500.0));
        assert_eq!(t_30.rotation, 90.0);
        assert_eq!(t_30.scale, Vec2::new(200.0, 200.0));

        // Local origin (0, 0) maps to (500, 500)
        assert_eq!(t_30.local_to_world_point(Vec2::ZERO), Vec2::new(500.0, 500.0));

        // Point (100, 0) in local space: scaled to (200, 0), rotated 90 deg clockwise -> (0, 200), translated by (500, 500) -> (500, 700)
        let p_transformed = t_30.local_to_world_point(Vec2::new(100.0, 0.0));
        assert!((p_transformed.x - 500.0).abs() < 1e-4);
        assert!((p_transformed.y - 700.0).abs() < 1e-4);
    }

    #[test]
    fn test_hierarchical_transform_evaluation_with_animated_parent_and_child() {
        use project::{Keyframe, Vec2};

        let fps = 30.0;
        let mut comp = Composition::hd_1080p_30fps("comp_anim_hier", "Hierarchy Animation", 5.0);
        let tc0 = TimeCode::from_frames(0, fps);
        let tc60 = TimeCode::from_frames(60, fps);

        // Parent Layer: moves horizontally from (0, 100) to (600, 100) over 60 frames (2.0s)
        let mut parent = Layer::solid("parent_node", "Parent", Color::RED, 200, 200, tc0, tc60);
        parent.transform.anchor_point.set_value(Vec2::ZERO);
        parent.transform.position.add_keyframe(Keyframe::linear(tc0, Vec2::new(0.0, 100.0)));
        parent.transform.position.add_keyframe(Keyframe::linear(tc60, Vec2::new(600.0, 100.0)));

        // Child Layer: parented to parent, positioned locally at (50, 0), rotates 0 -> 360 deg over 60 frames
        let mut child = Layer::solid("child_node", "Child", Color::BLUE, 50, 50, tc0, tc60);
        child.set_parent(Some("parent_node"));
        child.transform.anchor_point.set_value(Vec2::ZERO);
        child.transform.position.set_value(Vec2::new(50.0, 0.0));
        child.transform.rotation.add_keyframe(Keyframe::linear(tc0, 0.0));
        child.transform.rotation.add_keyframe(Keyframe::linear(tc60, 360.0));

        comp.add_layer(parent).unwrap();
        comp.add_layer(child).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();

        // 1. Frame 0: Parent at (0, 100), Child local (50, 0) -> Child world (50, 100)
        let stack_0 = evaluator.evaluate(&graph, &tc0);
        let child_t0 = stack_0.get_transform("child_node").unwrap();
        assert_eq!(child_t0.local_to_world_point(Vec2::ZERO), Vec2::new(50.0, 100.0));

        // 2. Frame 30 (Midpoint, 1.0s):
        // Parent has moved to (300, 100)
        // Child rotation is 180 degrees
        // Child local (0, 0): offset (50, 0) from parent. In parent space: (50, 0).
        // Parent rotation is 0, so parent world origin + (50, 0) = (350, 100)
        let stack_30 = evaluator.evaluate(&graph, &TimeCode::from_frames(30, fps));
        let parent_t30 = stack_30.get_transform("parent_node").unwrap();
        let child_t30 = stack_30.get_transform("child_node").unwrap();
        assert_eq!(parent_t30.position, Vec2::new(300.0, 100.0));
        assert!((child_t30.rotation - 180.0).abs() < 1e-4);
        assert_eq!(child_t30.local_to_world_point(Vec2::ZERO), Vec2::new(350.0, 100.0));

        // Point (10, 0) in child space: rotated 180 deg in child -> (-10, 0) relative to child pos (50, 0) -> (40, 0) in parent space
        // Translated by parent pos (300, 100) -> (340, 100)
        let child_pt_world = child_t30.local_to_world_point(Vec2::new(10.0, 0.0));
        assert!((child_pt_world.x - 340.0).abs() < 1e-4);
        assert!((child_pt_world.y - 100.0).abs() < 1e-4);

        // 3. Frame 60: Parent has reached (600, 100), Child rotation completed 360 deg
        let stack_60 = evaluator.evaluate(&graph, &tc60);
        // At exact out_point 60, layers are inactive
        let l_parent = stack_60.get_layer("parent_node").unwrap();
        assert!(!l_parent.is_active);

        // Frame 59 (active):
        let stack_59 = evaluator.evaluate(&graph, &TimeCode::from_frames(59, fps));
        let l_child_59 = stack_59.get_layer("child_node").unwrap();
        assert!(l_child_59.is_active);
    }

    #[test]
    fn test_basic_nested_composition_evaluation() {
        let mut project = Project::with_defaults("Basic Nesting Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Child Comp B: 800x600, duration 150 frames (5.0s)
        let mut comp_b = Composition::new("comp_b", "Child Comp B", 800, 600, 30.0, tc150);
        let b_bg = Layer::solid("b_bg", "B Background", Color::RED, 800, 600, tc0, tc150);
        let b_text = Layer::text(
            "b_text",
            "B Title",
            "Nested Title",
            "Inter",
            32.0,
            Color::WHITE,
            tc0,
            tc150,
        );
        comp_b.add_layer(b_bg).unwrap();
        comp_b.add_layer(b_text).unwrap();

        // Parent Comp A: 1920x1080, duration 150 frames (5.0s)
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Parent Comp A", 5.0);
        let nested_layer = Layer::nested_composition("nested_b", "Precomp B", "comp_b", tc0, tc150);
        comp_a.add_layer(nested_layer).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .expect("Evaluate comp_a");

        assert_eq!(stack_a.composition_id, "comp_a");
        assert_eq!(stack_a.evaluated_layers.len(), 1);
        assert!(stack_a.has_nested_compositions());

        let l_nested = stack_a.get_layer("nested_b").unwrap();
        assert!(l_nested.is_nested_composition());
        assert!(l_nested.is_active);
        assert!(l_nested.is_visible);

        let nested_eval = l_nested.nested_evaluation().expect("Has nested evaluation");
        assert_eq!(nested_eval.composition_id, "comp_b");
        assert_eq!(nested_eval.width, 800);
        assert_eq!(nested_eval.height, 600);
        assert_eq!(nested_eval.resolved_time.frames(), 0);

        let inner_stack = &nested_eval.inner_stack;
        assert_eq!(inner_stack.composition_id, "comp_b");
        assert_eq!(inner_stack.evaluated_layers.len(), 2);
        assert!(inner_stack.get_layer("b_bg").unwrap().is_active);
        assert!(inner_stack.get_layer("b_text").unwrap().is_active);

        // Test canvas bounds in root
        let bounds_root = nested_eval.canvas_bounds_in_root();
        assert_eq!(bounds_root.min, Vec2::ZERO);
        assert_eq!(bounds_root.max, Vec2::new(800.0, 600.0));
    }

    #[test]
    fn test_time_offset_alignment_in_nested_composition() {
        let mut project = Project::with_defaults("Timing Alignment Project");
        let fps = 30.0;
        let tc0 = TimeCode::from_frames(0, fps);
        let tc60 = TimeCode::from_frames(60, fps);
        let tc120 = TimeCode::from_frames(120, fps);

        // Child Comp B: duration 120 frames (4.0s)
        // Layer 1: active on [0, 60)
        // Layer 2: active on [60, 120)
        let mut comp_b = Composition::new("comp_b", "Child B", 1920, 1080, fps, tc120);
        let l1 = Layer::solid("b_l1", "First Half", Color::RED, 100, 100, tc0, tc60);
        let l2 = Layer::solid("b_l2", "Second Half", Color::BLUE, 100, 100, tc60, tc120);
        comp_b.add_layer(l1).unwrap();
        comp_b.add_layer(l2).unwrap();

        // Parent Comp A:
        // Nesting layer starts at frame 30 with in_point=30, out_point=150, start_offset = 10 frames
        // Expected formula: t_nested = t_parent - t_in + t_offset
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Parent A", 10.0);
        let nested_layer = Layer::nested_composition(
            "nested_b",
            "Precomp B",
            "comp_b",
            TimeCode::from_frames(30, fps),
            TimeCode::from_frames(150, fps),
        )
        .with_start_offset(TimeCode::from_frames(10, fps));

        comp_a.add_layer(nested_layer).unwrap();
        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();

        // 1. Parent Frame 29 (before in_point 30): nesting layer is inactive
        let stack_29 = evaluator
            .evaluate_composition(&project, "comp_a", &TimeCode::from_frames(29, fps))
            .unwrap();
        let layer_29 = stack_29.get_layer("nested_b").unwrap();
        assert!(!layer_29.is_active);
        assert!(layer_29.nested_composition.is_none());

        // 2. Parent Frame 30 (at in_point 30):
        // t_nested = 30 - 30 + 10 = 10 frames
        let stack_30 = evaluator
            .evaluate_composition(&project, "comp_a", &TimeCode::from_frames(30, fps))
            .unwrap();
        let nested_eval_30 = stack_30
            .get_layer("nested_b")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(nested_eval_30.resolved_time.frames(), 10);
        // At frame 10: b_l1 is active [0, 60), b_l2 is inactive [60, 120)
        assert!(nested_eval_30.inner_stack.get_layer("b_l1").unwrap().is_active);
        assert!(!nested_eval_30.inner_stack.get_layer("b_l2").unwrap().is_active);

        // 3. Parent Frame 90:
        // t_nested = 90 - 30 + 10 = 70 frames
        let stack_90 = evaluator
            .evaluate_composition(&project, "comp_a", &TimeCode::from_frames(90, fps))
            .unwrap();
        let nested_eval_90 = stack_90
            .get_layer("nested_b")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(nested_eval_90.resolved_time.frames(), 70);
        // At frame 70: b_l1 is inactive, b_l2 is active [60, 120)
        assert!(!nested_eval_90.inner_stack.get_layer("b_l1").unwrap().is_active);
        assert!(nested_eval_90.inner_stack.get_layer("b_l2").unwrap().is_active);
    }

    #[test]
    fn test_time_stretch_and_speed_factor_evaluation() {
        let mut project = Project::with_defaults("Time Stretch Project");
        let fps = 30.0;
        let tc0 = TimeCode::from_frames(0, fps);
        let tc300 = TimeCode::from_frames(300, fps);

        let mut comp_child = Composition::new("comp_child", "Child", 1920, 1080, fps, tc300);

        comp_child
            .add_layer(Layer::solid("c_solid", "Solid", Color::WHITE, 100, 100, tc0, tc300))
            .unwrap();

        // 1. Slow-motion: 50% speed (time_stretch = 0.5)
        let mut comp_slow = Composition::hd_1080p_30fps("comp_slow", "Slow-Mo", 10.0);
        let slow_layer =
            Layer::nested_composition("slow_layer", "Slow Precomp", "comp_child", tc0, tc300)
                .with_time_stretch(0.5);
        comp_slow.add_layer(slow_layer).unwrap();

        // 2. Fast-forward: 200% double speed (time_stretch = 2.0)
        let mut comp_fast = Composition::hd_1080p_30fps("comp_fast", "Fast-Forward", 10.0);
        let fast_layer =
            Layer::nested_composition("fast_layer", "Fast Precomp", "comp_child", tc0, tc300)
                .with_time_stretch(2.0);
        comp_fast.add_layer(fast_layer).unwrap();

        // 3. Reverse playback: time_stretch = -1.0 with start_offset = 120 frames
        let mut comp_rev = Composition::hd_1080p_30fps("comp_rev", "Reverse", 10.0);
        let rev_layer =
            Layer::nested_composition("rev_layer", "Reverse Precomp", "comp_child", tc0, tc300)
                .with_time_stretch(-1.0)
                .with_start_offset(TimeCode::from_frames(120, fps));
        comp_rev.add_layer(rev_layer).unwrap();

        project.add_composition(comp_child).unwrap();
        project.add_composition(comp_slow).unwrap();
        project.add_composition(comp_fast).unwrap();
        project.add_composition(comp_rev).unwrap();

        let evaluator = LayerStackEvaluator::new();

        // Test Slow-motion (50%):
        // Parent at frame 40 -> child at 20 frames
        let stack_slow_40 = evaluator
            .evaluate_composition(&project, "comp_slow", &TimeCode::from_frames(40, fps))
            .unwrap();
        let eval_slow_40 = stack_slow_40
            .get_layer("slow_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_slow_40.resolved_time.frames(), 20);

        // Parent at frame 100 -> child at 50 frames
        let stack_slow_100 = evaluator
            .evaluate_composition(&project, "comp_slow", &TimeCode::from_frames(100, fps))
            .unwrap();
        let eval_slow_100 = stack_slow_100
            .get_layer("slow_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_slow_100.resolved_time.frames(), 50);

        // Test Double speed (200%):
        // Parent at frame 20 -> child at 40 frames
        let stack_fast_20 = evaluator
            .evaluate_composition(&project, "comp_fast", &TimeCode::from_frames(20, fps))
            .unwrap();
        let eval_fast_20 = stack_fast_20
            .get_layer("fast_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_fast_20.resolved_time.frames(), 40);

        // Parent at frame 50 -> child at 100 frames
        let stack_fast_50 = evaluator
            .evaluate_composition(&project, "comp_fast", &TimeCode::from_frames(50, fps))
            .unwrap();
        let eval_fast_50 = stack_fast_50
            .get_layer("fast_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_fast_50.resolved_time.frames(), 100);

        // Test Reverse playback (-100%):
        // Parent at frame 0 -> child at 120 frames
        let stack_rev_0 = evaluator
            .evaluate_composition(&project, "comp_rev", &TimeCode::from_frames(0, fps))
            .unwrap();
        let eval_rev_0 = stack_rev_0
            .get_layer("rev_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_rev_0.resolved_time.frames(), 120);

        // Parent at frame 30 -> child at 90 frames (120 - 30)
        let stack_rev_30 = evaluator
            .evaluate_composition(&project, "comp_rev", &TimeCode::from_frames(30, fps))
            .unwrap();
        let eval_rev_30 = stack_rev_30
            .get_layer("rev_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_rev_30.resolved_time.frames(), 90);

        // Parent at frame 100 -> child at 20 frames (120 - 100)
        let stack_rev_100 = evaluator
            .evaluate_composition(&project, "comp_rev", &TimeCode::from_frames(100, fps))
            .unwrap();
        let eval_rev_100 = stack_rev_100
            .get_layer("rev_layer")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_rev_100.resolved_time.frames(), 20);
    }

    #[test]
    fn test_spatial_transform_cascading_across_nesting_boundaries() {
        use project::Vec2;

        let mut project = Project::with_defaults("Spatial Cascading Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Child Comp B: 400x300
        // Contains an inner solid (100x100) positioned at (50, 50)
        let mut comp_b = Composition::new("comp_b", "Child Comp B", 400, 300, 30.0, tc150);
        let mut inner_layer =
            Layer::solid("inner_rect", "Inner Rect", Color::RED, 100, 100, tc0, tc150);
        inner_layer.transform.anchor_point.set_value(Vec2::ZERO);
        inner_layer.transform.position.set_value(Vec2::new(50.0, 50.0));
        comp_b.add_layer(inner_layer).unwrap();

        // Parent Comp A: 1920x1080
        // Nesting layer has:
        // Position: (500, 400)
        // Rotation: 90 degrees clockwise
        // Scale: 200% (2.0x)
        // Anchor point: (0, 0)
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Parent Comp A", 5.0);
        let mut nesting_layer =
            Layer::nested_composition("nesting_b", "Precomp B", "comp_b", tc0, tc150);
        nesting_layer.transform.anchor_point.set_value(Vec2::ZERO);
        nesting_layer.transform.position.set_value(Vec2::new(500.0, 400.0));
        nesting_layer.transform.rotation.set_value(90.0);
        nesting_layer.transform.scale.set_value(Vec2::new(200.0, 200.0));
        comp_a.add_layer(nesting_layer).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .expect("Evaluate");

        let l_nested = stack_a.get_layer("nesting_b").unwrap();
        let nested_eval = l_nested.nested_evaluation().unwrap();

        // 1. Mapping a point from Comp B (50, 50) to Comp A:
        // In Comp A, nesting layer scales by 2 -> (100, 100)
        // Rotated 90 deg clockwise around (0,0):
        // x' = -100, y' = 100
        // Translated by (500, 400):
        // (500 - 100, 400 + 100) = (400, 500)
        let pt_in_b = Vec2::new(50.0, 50.0);
        let pt_in_a = nested_eval.inner_to_root_point(pt_in_b);
        assert!((pt_in_a.x - 400.0).abs() < 1e-4);
        assert!((pt_in_a.y - 500.0).abs() < 1e-4);

        // 2. Inverting point from Comp A back to Comp B:
        let pt_recovered = nested_eval.root_to_inner_point(pt_in_a).unwrap();
        assert!((pt_recovered.x - pt_in_b.x).abs() < 1e-4);
        assert!((pt_recovered.y - pt_in_b.y).abs() < 1e-4);

        // 3. Concatenated matrix check for inner_layer:
        let concatenated_matrix = nested_eval
            .inner_layer_world_matrix("inner_rect")
            .expect("inner_layer matrix");
        let inner_origin_root = concatenated_matrix.transform_point(Vec2::ZERO);
        assert!((inner_origin_root.x - 400.0).abs() < 1e-4);
        assert!((inner_origin_root.y - 500.0).abs() < 1e-4);

        // 4. Inner layer root bounds check:
        // Inner layer is 100x100 at (50, 50).
        // Corners in Comp B: (50, 50), (150, 50), (150, 150), (50, 150)
        // In Comp A:
        // (50, 50) -> (400, 500)
        // (150, 50) -> scale 2: (300, 100) -> rot 90: (-100, 300) -> trans: (400, 700)
        // (150, 150) -> scale 2: (300, 300) -> rot 90: (-300, 300) -> trans: (200, 700)
        // (50, 150) -> scale 2: (100, 300) -> rot 90: (-300, 100) -> trans: (200, 500)
        // AABB in Comp A: min (200, 500), max (400, 700)
        let inner_root_bounds = nested_eval
            .inner_layer_root_bounds("inner_rect", 100.0, 100.0)
            .unwrap();
        assert!((inner_root_bounds.min.x - 200.0).abs() < 1e-4);
        assert!((inner_root_bounds.min.y - 500.0).abs() < 1e-4);
        assert!((inner_root_bounds.max.x - 400.0).abs() < 1e-4);
        assert!((inner_root_bounds.max.y - 700.0).abs() < 1e-4);
    }

    #[test]
    fn test_multi_level_deep_nesting() {
        let mut project = Project::with_defaults("Deep Nesting Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Comp C (innermost): has leaf solid at (10, 20)
        let mut comp_c = Composition::new("comp_c", "Comp C", 500, 500, 30.0, tc150);
        let mut leaf = Layer::solid("leaf_solid", "Leaf Solid", Color::GREEN, 50, 50, tc0, tc150);
        leaf.transform.anchor_point.set_value(Vec2::ZERO);
        leaf.transform.position.set_value(Vec2::new(10.0, 20.0));
        comp_c.add_layer(leaf).unwrap();

        // Comp B: nests Comp C at (50, 60)
        let mut comp_b = Composition::new("comp_b", "Comp B", 800, 800, 30.0, tc150);

        let mut layer_c = Layer::nested_composition("nest_c", "Nested C", "comp_c", tc0, tc150);
        layer_c.transform.anchor_point.set_value(Vec2::ZERO);
        layer_c.transform.position.set_value(Vec2::new(50.0, 60.0));
        comp_b.add_layer(layer_c).unwrap();

        // Comp A: nests Comp B at (100, 200)
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Comp A", 5.0);
        let mut layer_b = Layer::nested_composition("nest_b", "Nested B", "comp_b", tc0, tc150);
        layer_b.transform.anchor_point.set_value(Vec2::ZERO);
        layer_b.transform.position.set_value(Vec2::new(100.0, 200.0));
        comp_a.add_layer(layer_b).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();
        project.add_composition(comp_c).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .expect("Evaluate deep nesting");

        // Test hierarchical deep query:
        let deep_layer = stack_a
            .get_layer_deep(&["nest_b", "nest_c", "leaf_solid"])
            .expect("Find deep leaf solid");
        assert_eq!(deep_layer.name, "Leaf Solid");

        // Test flattened render list:
        let flattened = stack_a.flattened_render_list();
        assert_eq!(flattened.len(), 1);

        let item = &flattened[0];
        assert_eq!(item.layer_id, "leaf_solid");
        assert_eq!(item.layer_path, vec!["nest_b", "nest_c", "leaf_solid"]);
        assert_eq!(item.nesting_depth, 2);

        // Accumulated position in Comp A:
        // Leaf (10, 20) + Comp C in B (50, 60) + Comp B in A (100, 200) = (160, 280)
        let root_pos = item.local_to_root_point(Vec2::ZERO);
        assert_eq!(root_pos, Vec2::new(160.0, 280.0));
    }

    #[test]
    fn test_circular_nesting_and_missing_composition_error_handling() {
        let mut project = Project::with_defaults("Error Handling Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // 1. Missing composition reference:
        let mut comp_missing = Composition::hd_1080p_30fps("comp_miss", "Missing Comp Test", 5.0);
        comp_missing
            .add_layer(Layer::nested_composition(
                "nest_bad",
                "Non Existent",
                "non_existent_comp_id",
                tc0,
                tc150,
            ))
            .unwrap();
        project.add_composition(comp_missing).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let err_missing = evaluator
            .evaluate_composition(&project, "comp_miss", &tc0)
            .unwrap_err();
        assert!(matches!(
            err_missing,
            SceneGraphError::CompositionNotFound(ref id) if id == "non_existent_comp_id"
        ));

        // 2. Circular nesting: Comp A -> Comp B -> Comp A
        let mut comp_cycle_a = Composition::hd_1080p_30fps("cycle_a", "Cycle A", 5.0);
        let mut comp_cycle_b = Composition::hd_1080p_30fps("cycle_b", "Cycle B", 5.0);

        comp_cycle_a
            .add_layer(Layer::nested_composition(
                "nest_b", "Nests B", "cycle_b", tc0, tc150,
            ))
            .unwrap();
        comp_cycle_b
            .add_layer(Layer::nested_composition(
                "nest_a", "Nests A", "cycle_a", tc0, tc150,
            ))
            .unwrap();

        let mut cycle_proj = Project::with_defaults("Cycle Proj");
        cycle_proj.add_composition(comp_cycle_a).unwrap();
        cycle_proj.add_composition(comp_cycle_b).unwrap();

        let err_cycle = evaluator
            .evaluate_composition(&cycle_proj, "cycle_a", &tc0)
            .unwrap_err();
        assert!(matches!(
            err_cycle,
            SceneGraphError::CircularNestedComposition {
                ref composition_id,
                ..
            } if composition_id == "cycle_a"
        ));

        // 3. Max nesting depth exceeded:
        // Set max_nesting_depth = 2 on a 4-level deep chain (c1 -> c2 -> c3 -> c4)
        let strict_evaluator = LayerStackEvaluator::new().with_max_nesting_depth(2);

        let mut c4 = Composition::hd_1080p_30fps("c4", "C4", 5.0);
        c4.add_layer(Layer::solid("s", "S", Color::RED, 10, 10, tc0, tc150))
            .unwrap();

        let mut c3 = Composition::hd_1080p_30fps("c3", "C3", 5.0);
        c3.add_layer(Layer::nested_composition("n4", "N4", "c4", tc0, tc150))
            .unwrap();

        let mut c2 = Composition::hd_1080p_30fps("c2", "C2", 5.0);
        c2.add_layer(Layer::nested_composition("n3", "N3", "c3", tc0, tc150))
            .unwrap();

        let mut c1 = Composition::hd_1080p_30fps("c1", "C1", 5.0);
        c1.add_layer(Layer::nested_composition("n2", "N2", "c2", tc0, tc150))
            .unwrap();

        let mut depth_proj = Project::with_defaults("Depth Proj");
        depth_proj.add_composition(c1).unwrap();
        depth_proj.add_composition(c2).unwrap();
        depth_proj.add_composition(c3).unwrap();
        depth_proj.add_composition(c4).unwrap();

        let err_depth = strict_evaluator
            .evaluate_composition(&depth_proj, "c1", &tc0)
            .unwrap_err();
        assert!(matches!(
            err_depth,
            SceneGraphError::MaxNestingDepthExceeded {
                depth: 3,
                max_depth: 2
            }
        ));
    }

    #[test]
    fn test_flattened_composite_render_order_generation() {
        let mut project = Project::with_defaults("Composite Order Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Comp B:
        // Stack index 0: B Top (upper layer)
        // Stack index 1: B Bottom (lower layer)
        // Composite render order in B: B Bottom -> B Top
        let mut comp_b = Composition::new("comp_b", "Child B", 800, 600, 30.0, tc150);
        let b_top = Layer::solid("b_top", "B Top", Color::WHITE, 100, 100, tc0, tc150);
        let b_bot = Layer::solid("b_bot", "B Bottom", Color::BLACK, 100, 100, tc0, tc150);
        comp_b.add_layer(b_top).unwrap();
        comp_b.add_layer(b_bot).unwrap();

        // Comp A:
        // Stack index 0: A Top (foreground)
        // Stack index 1: Precomp B (nested middle)
        // Stack index 2: A Bottom (background)
        // Composite render order in A: A Bottom -> Precomp B -> A Top
        // With Precomp B expanded: A Bottom -> B Bottom -> B Top -> A Top
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Parent A", 5.0);
        let a_top = Layer::solid("a_top", "A Top", Color::RED, 1920, 1080, tc0, tc150);
        let a_mid = Layer::nested_composition("a_mid", "Precomp B", "comp_b", tc0, tc150);
        let a_bot = Layer::solid("a_bot", "A Bottom", Color::BLUE, 1920, 1080, tc0, tc150);

        comp_a.add_layer(a_top).unwrap();
        comp_a.add_layer(a_mid).unwrap();
        comp_a.add_layer(a_bot).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .unwrap();

        // Check local render list for Comp A: bottom to top
        assert_eq!(stack_a.render_list, vec!["a_bot", "a_mid", "a_top"]);

        // Check flattened render list:
        let flattened = stack_a.flattened_render_list();
        let flattened_ids: Vec<&str> = flattened.iter().map(|l| l.layer_id.as_str()).collect();
        assert_eq!(flattened_ids, vec!["a_bot", "b_bot", "b_top", "a_top"]);

        // Check render pass descriptors:
        let passes = stack_a.collect_render_passes();
        assert_eq!(passes.len(), 2);
        // Leaf pass first (Comp B)
        assert_eq!(passes[0].composition_id, "comp_b");
        assert_eq!(passes[0].layer_ids, vec!["b_bot", "b_top"]);
        // Root pass second (Comp A)
        assert_eq!(passes[1].composition_id, "comp_a");
        assert_eq!(passes[1].layer_ids, vec!["a_bot", "a_mid", "a_top"]);
    }

    #[test]
    fn test_animated_time_remapping_on_nested_composition() {
        use project::Keyframe;

        let mut project = Project::with_defaults("Time Remap Project");
        let fps = 30.0;
        let tc0 = TimeCode::from_frames(0, fps);
        let tc30 = TimeCode::from_frames(30, fps);
        let tc60 = TimeCode::from_frames(60, fps);
        let tc90 = TimeCode::from_frames(90, fps);
        let tc300 = TimeCode::from_frames(300, fps);

        let mut comp_child = Composition::new("comp_child", "Child", 1920, 1080, fps, tc300);
        comp_child
            .add_layer(Layer::solid("c_solid", "Solid", Color::RED, 100, 100, tc0, tc90))
            .unwrap();

        // Parent Comp:
        // Nested comp layer has animated time remapping:
        // Frame 0 -> 0.0s (frame 0)
        // Frame 30 -> 3.0s (frame 90 in child, fast forward)
        // Frame 60 -> 1.0s (frame 30 in child, rewound)
        let mut comp_parent = Composition::hd_1080p_30fps("comp_parent", "Parent", 5.0);
        let mut remapping = project::Property::new("Time Remap", 0.0);
        remapping.add_keyframe(Keyframe::linear(tc0, 0.0));
        remapping.add_keyframe(Keyframe::linear(tc30, 3.0));
        remapping.add_keyframe(Keyframe::linear(tc60, 1.0));

        let nested_layer =
            Layer::nested_composition("nest_remap", "Precomp", "comp_child", tc0, tc90)
                .with_time_remapping(remapping);
        comp_parent.add_layer(nested_layer).unwrap();

        project.add_composition(comp_child).unwrap();
        project.add_composition(comp_parent).unwrap();

        let evaluator = LayerStackEvaluator::new();

        // 1. Frame 0 -> 0.0s = 0 frames
        let s0 = evaluator
            .evaluate_composition(&project, "comp_parent", &tc0)
            .unwrap();
        let eval0 = s0
            .get_layer("nest_remap")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval0.resolved_time.frames(), 0);

        // 2. Frame 15 (halfway between 0.0s and 3.0s) -> 1.5s = 45 frames
        let s15 = evaluator
            .evaluate_composition(&project, "comp_parent", &TimeCode::from_frames(15, fps))
            .unwrap();
        let eval15 = s15
            .get_layer("nest_remap")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval15.resolved_time.frames(), 45);

        // 3. Frame 30 -> 3.0s = 90 frames
        let s30 = evaluator
            .evaluate_composition(&project, "comp_parent", &tc30)
            .unwrap();
        let eval30 = s30
            .get_layer("nest_remap")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval30.resolved_time.frames(), 90);

        // 4. Frame 45 (halfway between 3.0s and 1.0s) -> 2.0s = 60 frames
        let s45 = evaluator
            .evaluate_composition(&project, "comp_parent", &TimeCode::from_frames(45, fps))
            .unwrap();
        let eval45 = s45
            .get_layer("nest_remap")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval45.resolved_time.frames(), 60);

        // 5. Frame 60 -> 1.0s = 30 frames
        let s60 = evaluator
            .evaluate_composition(&project, "comp_parent", &tc60)
            .unwrap();
        let eval60 = s60
            .get_layer("nest_remap")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval60.resolved_time.frames(), 30);
    }

    #[test]
    fn test_nested_composition_loop_modes_and_bounds_clamping() {
        use project::LoopMode;

        let mut project = Project::with_defaults("Loop Modes Project");
        let fps = 30.0;
        let tc0 = TimeCode::from_frames(0, fps);
        let tc60 = TimeCode::from_frames(60, fps);
        let tc300 = TimeCode::from_frames(300, fps);

        // Child comp has duration of 60 frames (2.0s)
        let mut comp_child = Composition::new("comp_child", "Child", 1920, 1080, fps, tc60);
        comp_child
            .add_layer(Layer::solid("c_solid", "Solid", Color::RED, 100, 100, tc0, tc60))
            .unwrap();

        // 1. LoopMode::Once (clamping to duration 60)
        let mut comp_once = Composition::hd_1080p_30fps("comp_once", "Once", 10.0);
        let layer_once =
            Layer::nested_composition("nest_once", "Once", "comp_child", tc0, tc300)
                .with_loop_mode(LoopMode::Once);
        comp_once.add_layer(layer_once).unwrap();

        // 2. LoopMode::Loop (wraps around modulo 60)
        let mut comp_loop = Composition::hd_1080p_30fps("comp_loop", "Loop", 10.0);
        let layer_loop =
            Layer::nested_composition("nest_loop", "Loop", "comp_child", tc0, tc300)
                .with_loop_mode(LoopMode::Loop);
        comp_loop.add_layer(layer_loop).unwrap();

        // 3. LoopMode::PingPong (bounces between 0 and 60 with period 120)
        let mut comp_ping = Composition::hd_1080p_30fps("comp_ping", "PingPong", 10.0);
        let layer_ping =
            Layer::nested_composition("nest_ping", "PingPong", "comp_child", tc0, tc300)
                .with_loop_mode(LoopMode::PingPong);
        comp_ping.add_layer(layer_ping).unwrap();

        project.add_composition(comp_child).unwrap();
        project.add_composition(comp_once).unwrap();
        project.add_composition(comp_loop).unwrap();
        project.add_composition(comp_ping).unwrap();

        let evaluator = LayerStackEvaluator::new();

        // At parent frame 75 (past child duration of 60 frames):
        // 1. LoopMode::Once -> clamps to duration 60
        let s_once = evaluator
            .evaluate_composition(&project, "comp_once", &TimeCode::from_frames(75, fps))
            .unwrap();
        let eval_once = s_once
            .get_layer("nest_once")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_once.resolved_time.frames(), 60);

        // 2. LoopMode::Loop -> wraps: 75 % 60 = 15
        let s_loop = evaluator
            .evaluate_composition(&project, "comp_loop", &TimeCode::from_frames(75, fps))
            .unwrap();
        let eval_loop = s_loop
            .get_layer("nest_loop")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_loop.resolved_time.frames(), 15);

        // 3. LoopMode::PingPong -> period 120: 75 > 60 -> 120 - 75 = 45
        let s_ping = evaluator
            .evaluate_composition(&project, "comp_ping", &TimeCode::from_frames(75, fps))
            .unwrap();
        let eval_ping = s_ping
            .get_layer("nest_ping")
            .unwrap()
            .nested_evaluation()
            .unwrap();
        assert_eq!(eval_ping.resolved_time.frames(), 45);
    }

    #[test]
    fn test_nested_composition_opacity_cascading() {
        let mut project = Project::with_defaults("Opacity Cascade Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Child comp has a solid with 80% opacity
        let mut comp_b = Composition::new("comp_b", "Child B", 800, 600, 30.0, tc150);

        let mut inner_solid =
            Layer::solid("b_solid", "B Solid", Color::WHITE, 800, 600, tc0, tc150);
        inner_solid.opacity.set_value(80.0);
        comp_b.add_layer(inner_solid).unwrap();

        // Parent comp nesting layer has 50% opacity
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Parent A", 5.0);
        let mut nesting_layer =
            Layer::nested_composition("nest_b", "Precomp B", "comp_b", tc0, tc150);
        nesting_layer.opacity.set_value(50.0);
        comp_a.add_layer(nesting_layer).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .unwrap();

        let flattened = stack_a.flattened_render_list();
        assert_eq!(flattened.len(), 1);

        // 0.5 * 0.8 = 0.4 (40%)
        let combined_opacity = flattened[0].combined_opacity;
        assert!((combined_opacity - 0.4).abs() < 1e-5);
    }

    #[test]
    fn test_inner_layer_root_bounds_exactness_under_opposing_rotations() {
        let mut project = Project::with_defaults("Rotation Exactness Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Child Comp B (400x400):
        // Inner layer is a 100x100 solid positioned at (150, 150) rotated 45 degrees.
        let mut comp_b = Composition::new("comp_b", "Comp B", 400, 400, 30.0, tc150);
        let mut inner_layer =
            Layer::solid("inner_rect", "Rotated Inner", Color::RED, 100, 100, tc0, tc150);
        inner_layer.transform.anchor_point.set_value(Vec2::ZERO);
        inner_layer.transform.position.set_value(Vec2::new(150.0, 150.0));
        inner_layer.transform.rotation.set_value(45.0);
        comp_b.add_layer(inner_layer).unwrap();

        // Parent Comp A:
        // Nesting layer is positioned at (200, 200) and rotated -45 degrees.
        // Net rotation in Comp A viewport = 45 deg + (-45 deg) = 0 deg!
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Comp A", 5.0);
        let mut nesting_layer =
            Layer::nested_composition("nest_b", "Precomp B", "comp_b", tc0, tc150);
        nesting_layer.transform.anchor_point.set_value(Vec2::ZERO);
        nesting_layer.transform.position.set_value(Vec2::new(200.0, 200.0));
        nesting_layer.transform.rotation.set_value(-45.0);
        comp_a.add_layer(nesting_layer).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .expect("Evaluate");

        let l_nested = stack_a.get_layer("nest_b").unwrap();
        let nested_eval = l_nested.nested_evaluation().unwrap();

        // Under net rotation 0, the root AABB width and height must be exactly 100.0 (zero AABB bloat)
        let exact_bounds = nested_eval
            .inner_layer_root_bounds("inner_rect", 100.0, 100.0)
            .expect("Root bounds");
        assert!((exact_bounds.width() - 100.0).abs() < 1e-3);
        assert!((exact_bounds.height() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn test_nested_composition_as_track_matte_render_passes() {
        use project::TrackMatteMode;

        let mut project = Project::with_defaults("Matte Precomp Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Child Comp B (matte source shape)
        let mut comp_b = Composition::new("comp_b", "Matte Comp", 400, 400, 30.0, tc150);
        comp_b
            .add_layer(Layer::solid(
                "matte_shape",
                "Circle",
                Color::WHITE,
                400,
                400,
                tc0,
                tc150,
            ))
            .unwrap();

        // Parent Comp A:
        // Layer 0: "b_matte" (pre-comp of comp_b)
        // Layer 1: "video_clip" (uses "b_matte" as alpha track matte)
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Parent Comp", 5.0);
        let matte_layer = Layer::nested_composition("b_matte", "Matte Precomp", "comp_b", tc0, tc150);
        let mut video_layer =
            Layer::solid("video_clip", "Video Clip", Color::RED, 1920, 1080, tc0, tc150);
        video_layer.set_matte(TrackMatteMode::Alpha, Some("b_matte"));

        comp_a.add_layer(matte_layer).unwrap();
        comp_a.add_layer(video_layer).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        // 1. Default evaluator (consume_matte_sources = true):
        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .unwrap();

        // Main render list only has video_clip (matte was consumed)
        assert_eq!(stack_a.render_list, vec!["video_clip"]);

        // BUT collect_render_passes MUST still collect the offscreen pass for comp_b!
        let passes = stack_a.collect_render_passes();
        assert_eq!(passes.len(), 2);
        // Leaf pass first: comp_b
        assert_eq!(passes[0].composition_id, "comp_b");
        assert_eq!(passes[0].nesting_layer_id.as_deref(), Some("b_matte"));
        assert_eq!(passes[0].layer_ids, vec!["matte_shape"]);
        // Root pass second: comp_a
        assert_eq!(passes[1].composition_id, "comp_a");
        assert_eq!(passes[1].nesting_layer_id, None);
        assert_eq!(passes[1].layer_ids, vec!["video_clip"]);

        // 2. Non-consuming evaluator:
        let non_consuming = LayerStackEvaluator::new().with_consume_matte_sources(false);
        let stack_nc = non_consuming
            .evaluate_composition(&project, "comp_a", &tc0)
            .unwrap();
        let flattened = stack_nc.flattened_render_list();
        assert_eq!(flattened.len(), 2);
        // Inner layer of b_matte inherits is_matte_source = true
        let flat_matte = flattened
            .iter()
            .find(|l| l.layer_id == "matte_shape")
            .unwrap();
        assert!(flat_matte.is_matte_source);
    }

    #[test]
    fn test_nested_composition_parented_in_parent_and_child() {
        let mut project = Project::with_defaults("Cross Boundary Parenting Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Comp B:
        // Layer "b_parent" positioned at (30, 40)
        // Layer "b_leaf" parented to "b_parent" positioned locally at (5, 5)
        let mut comp_b = Composition::new("comp_b", "Comp B", 600, 600, 30.0, tc150);
        let mut b_parent = Layer::solid("b_parent", "B Parent", Color::BLUE, 50, 50, tc0, tc150);
        b_parent.transform.anchor_point.set_value(Vec2::ZERO);
        b_parent.transform.position.set_value(Vec2::new(30.0, 40.0));

        let mut b_leaf = Layer::solid("b_leaf", "B Leaf", Color::GREEN, 20, 20, tc0, tc150);
        b_leaf.transform.anchor_point.set_value(Vec2::ZERO);
        b_leaf.transform.position.set_value(Vec2::new(5.0, 5.0));
        b_leaf.set_parent(Some("b_parent"));

        comp_b.add_layer(b_parent).unwrap();
        comp_b.add_layer(b_leaf).unwrap();

        // Comp A:
        // Layer "a_null" positioned at (100, 200)
        // Layer "a_nest" nests Comp B, positioned at (10, 20), parented to "a_null"
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Comp A", 5.0);
        let mut a_null = Layer::solid("a_null", "A Null", Color::BLACK, 10, 10, tc0, tc150);
        a_null.transform.anchor_point.set_value(Vec2::ZERO);
        a_null.transform.position.set_value(Vec2::new(100.0, 200.0));

        let mut a_nest = Layer::nested_composition("a_nest", "Nested B", "comp_b", tc0, tc150);
        a_nest.transform.anchor_point.set_value(Vec2::ZERO);
        a_nest.transform.position.set_value(Vec2::new(10.0, 20.0));
        a_nest.set_parent(Some("a_null"));

        comp_a.add_layer(a_null).unwrap();
        comp_a.add_layer(a_nest).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new();
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .unwrap();

        // Accumulated position for b_leaf:
        // (100 + 10) + (30 + 5) = 145
        // (200 + 20) + (40 + 5) = 265
        let root_matrix = stack_a
            .deep_layer_root_matrix(&["a_nest", "b_leaf"])
            .expect("Leaf root matrix");
        let origin = root_matrix.transform_point(Vec2::ZERO);
        assert!((origin.x - 145.0).abs() < 1e-4);
        assert!((origin.y - 265.0).abs() < 1e-4);

        let flat_leaf = stack_a.get_flattened_layer("b_leaf").unwrap();
        assert_eq!(flat_leaf.local_to_root_point(Vec2::ZERO), Vec2::new(145.0, 265.0));
    }

    #[test]
    fn test_flattened_render_layer_matte_inheritance_and_path() {
        use project::TrackMatteMode;

        let mut project = Project::with_defaults("Matte Path Project");
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);

        // Comp B has internal track matte pairing
        let mut comp_b = Composition::new("comp_b", "Comp B", 500, 500, 30.0, tc150);
        let inner_matte = Layer::solid("m_src", "Matte Src", Color::WHITE, 50, 50, tc0, tc150);
        let mut inner_target = Layer::solid("m_tgt", "Matte Target", Color::RED, 50, 50, tc0, tc150);
        inner_target.set_matte(TrackMatteMode::Alpha, Some("m_src"));

        comp_b.add_layer(inner_matte).unwrap();
        comp_b.add_layer(inner_target).unwrap();

        // Comp A nests Comp B
        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Comp A", 5.0);
        let nest = Layer::nested_composition("nest_b", "Nest B", "comp_b", tc0, tc150);
        comp_a.add_layer(nest).unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        let evaluator = LayerStackEvaluator::new().with_consume_matte_sources(false);
        let stack_a = evaluator
            .evaluate_composition(&project, "comp_a", &tc0)
            .unwrap();

        let flat_target = stack_a.get_flattened_layer("m_tgt").unwrap();
        assert_eq!(flat_target.matte_mode, TrackMatteMode::Alpha);
        assert_eq!(flat_target.matte_source_id.as_deref(), Some("m_src"));
        assert_eq!(
            flat_target.matte_source_path,
            Some(vec!["nest_b".to_string(), "m_src".to_string()])
        );

        let flat_matte = stack_a.get_flattened_layer("m_src").unwrap();
        assert!(flat_matte.is_matte_source);
    }

    #[test]
    fn test_resolve_nested_time_nan_and_infinite_stretch_robustness() {
        use project::LoopMode;

        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc150 = TimeCode::from_frames(150, 30.0);
        let comp = Composition::hd_1080p_30fps("comp", "Comp", 5.0);

        // NaN time stretch
        let mut layer_nan = Layer::nested_composition("nan_layer", "NaN", "comp", tc0, tc150);
        layer_nan.time_stretch = f64::NAN;
        let node_nan = SceneNode::from_layer(&layer_nan, 0);
        let t_nan = resolve_nested_time(&node_nan, &tc0, &comp);
        assert_eq!(t_nan.frames(), 0);

        // Infinite time stretch
        let mut layer_inf = Layer::nested_composition("inf_layer", "Inf", "comp", tc0, tc150);
        layer_inf.time_stretch = f64::INFINITY;
        let node_inf = SceneNode::from_layer(&layer_inf, 0);
        let t_inf = resolve_nested_time(&node_inf, &tc0, &comp);
        assert_eq!(t_inf.frames(), 0);

        // Animated time remap returning non-finite value
        let mut layer_remap = Layer::nested_composition("remap_nan", "Remap", "comp", tc0, tc150);
        layer_remap.time_remapping = Some(project::Property::new("Remap", f64::NAN));
        layer_remap.loop_mode = LoopMode::Loop;
        let node_remap = SceneNode::from_layer(&layer_remap, 0);
        let t_remap = resolve_nested_time(&node_remap, &tc0, &comp);
        assert_eq!(t_remap.frames(), 0);
    }

    #[test]
    fn test_layer_effects_evaluation_over_time() {
        use project::{Effect, EffectType, Keyframe};

        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc100 = TimeCode::from_frames(100, 30.0);
        let mut comp = Composition::hd_1080p_30fps("comp_fx", "Effects Comp", 5.0);

        let mut layer = Layer::solid("layer_fx", "Solid", Color::RED, 1920, 1080, tc0, tc100);

        // Animated Gaussian Blur: radius 0.0 at frame 0 -> 50.0 at frame 50
        let mut blur_effect = Effect::gaussian_blur("fx_blur_anim", 0.0);
        if let EffectType::GaussianBlur { ref mut radius } = blur_effect.effect_type {
            radius.add_keyframe(Keyframe::linear(TimeCode::from_frames(0, 30.0), 0.0));
            radius.add_keyframe(Keyframe::linear(TimeCode::from_frames(50, 30.0), 50.0));
        }
        layer.add_effect(blur_effect);

        // Invert effect (initially disabled)
        let mut invert_effect = Effect::invert("fx_inv", 100.0);
        invert_effect.enabled = false;
        layer.add_effect(invert_effect);

        comp.add_layer(layer).unwrap();

        let graph = SceneGraph::from_composition(&comp).unwrap();
        let evaluator = LayerStackEvaluator::new();

        // 1. Evaluate at frame 0
        let eval0 = evaluator.evaluate(&graph, &TimeCode::from_frames(0, 30.0));
        let l0 = eval0.get_layer("layer_fx").expect("layer at frame 0");
        // Disabled effect should not be included in evaluated effects
        assert_eq!(l0.effects.len(), 1);
        assert_eq!(l0.effects[0].id, "fx_blur_anim");
        match &l0.effects[0].effect_type {
            EvaluatedEffectType::GaussianBlur { radius } => {
                assert_eq!(*radius, 0.0);
            }
            _ => panic!("Expected GaussianBlur"),
        }

        // 2. Evaluate at frame 25 (halfway)
        let eval25 = evaluator.evaluate(&graph, &TimeCode::from_frames(25, 30.0));
        let l25 = eval25.get_layer("layer_fx").unwrap();
        match &l25.effects[0].effect_type {
            EvaluatedEffectType::GaussianBlur { radius } => {
                assert!((*radius - 25.0).abs() < 1e-3);
            }
            _ => panic!("Expected GaussianBlur"),
        }

        // 3. Evaluate at frame 50 (end of keyframes)
        let eval50 = evaluator.evaluate(&graph, &TimeCode::from_frames(50, 30.0));
        let l50 = eval50.get_layer("layer_fx").unwrap();
        match &l50.effects[0].effect_type {
            EvaluatedEffectType::GaussianBlur { radius } => {
                assert_eq!(*radius, 50.0);
            }
            _ => panic!("Expected GaussianBlur"),
        }

        // 4. Enable invert and verify both effects are evaluated
        comp.get_layer_mut("layer_fx").unwrap().get_effect_mut("fx_inv").unwrap().enabled = true;
        let graph2 = SceneGraph::from_composition(&comp).unwrap();
        let eval_enabled = evaluator.evaluate(&graph2, &TimeCode::from_frames(50, 30.0));
        let l_both = eval_enabled.get_layer("layer_fx").unwrap();
        assert_eq!(l_both.effects.len(), 2);
        assert_eq!(l_both.effects[1].id, "fx_inv");
        match &l_both.effects[1].effect_type {
            EvaluatedEffectType::Invert { amount } => {
                assert_eq!(*amount, 100.0);
            }
            _ => panic!("Expected Invert"),
        }

        // 5. Verify FlattenedRenderLayer preserves evaluated effects
        let flat_list = eval_enabled.flattened_render_list();
        let flat_fx_layer = flat_list
            .iter()
            .find(|l| l.layer_id == "layer_fx")
            .expect("layer_fx in flattened list");
        assert_eq!(flat_fx_layer.effects.len(), 2);
        assert_eq!(flat_fx_layer.effects[0].id, "fx_blur_anim");
        assert_eq!(flat_fx_layer.effects[1].id, "fx_inv");
    }
}


