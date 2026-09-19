pub mod error;
pub mod evaluation;
pub mod graph;
pub mod node;

pub use error::SceneGraphError;
pub use evaluation::{EvaluatedLayer, EvaluatedStack, LayerStackEvaluator};
pub use graph::SceneGraph;
pub use node::SceneNode;

#[cfg(test)]
mod tests {
    use super::*;
    use project::{Color, Composition, Layer, Project, TimeCode};
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
}
