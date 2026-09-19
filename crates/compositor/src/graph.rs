use crate::error::SceneGraphError;
use crate::node::SceneNode;
use crate::transform::{EvaluatedTransform, TransformResolver};
use project::{Composition, Project, TimeCode};
use std::collections::{HashMap, HashSet, VecDeque};

/// A hierarchical scene graph representing the visual and transform relationships
/// within a composition.
///
/// The scene graph models two complementary ordering dimensions:
/// 1. **Evaluation Order (Topological Sort)**: Guarantees every parent transform is computed
///    before any of its dependent children.
/// 2. **Composite / Render Order (Visual Stacking)**: Determines the painter's algorithm order
///    (bottom-most layer rendered first, progressing upward to topmost layer).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneGraph {
    pub composition_id: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub duration: TimeCode,
    nodes: HashMap<String, SceneNode>,
    stack_order: Vec<String>,
    root_ids: Vec<String>,
}

impl SceneGraph {
    /// Build a SceneGraph from a `Composition`.
    ///
    /// Validates dimensions, frame rate, unique layer IDs, parent existence,
    /// self-parenting, and cyclic dependencies.
    pub fn from_composition(comp: &Composition) -> Result<Self, SceneGraphError> {
        if comp.width == 0 || comp.height == 0 {
            return Err(SceneGraphError::InvalidDimensions {
                width: comp.width,
                height: comp.height,
            });
        }
        if comp.frame_rate <= 0.0 || !comp.frame_rate.is_finite() {
            return Err(SceneGraphError::InvalidFrameRate(comp.frame_rate));
        }

        let mut nodes = HashMap::with_capacity(comp.layers.len());
        let mut stack_order = Vec::with_capacity(comp.layers.len());

        // Step 1: Collect nodes and check for duplicate IDs
        for (idx, layer) in comp.layers.iter().enumerate() {
            if nodes.contains_key(&layer.id) {
                return Err(SceneGraphError::DuplicateNodeId(layer.id.clone()));
            }
            let node = SceneNode::from_layer(layer, idx);
            nodes.insert(layer.id.clone(), node);
            stack_order.push(layer.id.clone());
        }

        // Step 2: Validate parents and wire up children lists
        let mut root_ids = Vec::new();
        for id in &stack_order {
            let parent_opt = nodes.get(id).unwrap().parent_id.clone();
            if let Some(ref parent_id) = parent_opt {
                if parent_id == id {
                    return Err(SceneGraphError::SelfParenting(id.clone()));
                }
                if !nodes.contains_key(parent_id) {
                    return Err(SceneGraphError::ParentNotFound {
                        node_id: id.clone(),
                        parent_id: parent_id.clone(),
                    });
                }
            } else {
                root_ids.push(id.clone());
            }
        }

        // Step 3: Populate children lists on parent nodes
        for id in &stack_order {
            let parent_opt = nodes.get(id).unwrap().parent_id.clone();
            if let Some(parent_id) = parent_opt {
                nodes.get_mut(&parent_id).unwrap().children_ids.push(id.clone());
            }
        }

        let graph = Self {
            composition_id: comp.id.clone(),
            width: comp.width,
            height: comp.height,
            frame_rate: comp.frame_rate,
            duration: comp.duration,
            nodes,
            stack_order,
            root_ids,
        };

        // Step 4: Cycle detection
        graph.detect_cycles()?;

        Ok(graph)
    }

    /// Build a SceneGraph for a specific composition within a `Project`.
    pub fn from_project(project: &Project, comp_id: &str) -> Result<Self, SceneGraphError> {
        let comp = project
            .get_composition(comp_id)
            .ok_or_else(|| SceneGraphError::CompositionNotFound(comp_id.to_string()))?;
        Self::from_composition(comp)
    }

    // --- Cycle Detection ---

    fn detect_cycles(&self) -> Result<(), SceneGraphError> {
        for node_id in &self.stack_order {
            let mut visited = HashSet::new();
            visited.insert(node_id.clone());
            let mut path = vec![node_id.clone()];
            let mut current = node_id.clone();

            while let Some(node) = self.nodes.get(&current) {
                if let Some(ref parent_id) = node.parent_id {
                    path.push(parent_id.clone());
                    if !visited.insert(parent_id.clone()) {
                        return Err(SceneGraphError::ParentCycleDetected {
                            node_id: node_id.clone(),
                            cycle: path,
                        });
                    }
                    current = parent_id.clone();
                } else {
                    break;
                }
            }
        }
        Ok(())
    }

    // --- Node Queries ---

    /// Return the total number of nodes in the scene graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the scene graph contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Retrieve an immutable reference to a scene node by ID.
    pub fn get_node(&self, id: &str) -> Option<&SceneNode> {
        self.nodes.get(id)
    }

    /// Retrieve a mutable reference to a scene node by ID.
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut SceneNode> {
        self.nodes.get_mut(id)
    }

    /// Retrieve the immediate parent node of a given node, if one exists.
    pub fn get_parent(&self, id: &str) -> Option<&SceneNode> {
        let node = self.get_node(id)?;
        let parent_id = node.parent_id.as_deref()?;
        self.get_node(parent_id)
    }

    /// Retrieve the immediate children nodes parented to a given node.
    pub fn get_children(&self, id: &str) -> Vec<&SceneNode> {
        let node = match self.get_node(id) {
            Some(n) => n,
            None => return Vec::new(),
        };
        node.children_ids
            .iter()
            .filter_map(|cid| self.get_node(cid))
            .collect()
    }

    /// Retrieve the ordered ancestor chain from immediate parent up to the root node.
    pub fn get_ancestor_chain(&self, id: &str) -> Vec<&SceneNode> {
        let mut ancestors = Vec::new();
        let mut current_id = id;
        while let Some(parent) = self.get_parent(current_id) {
            ancestors.push(parent);
            current_id = &parent.id;
        }
        ancestors
    }

    /// Retrieve all transitive descendant nodes in breadth-first order.
    pub fn get_descendants(&self, id: &str) -> Vec<&SceneNode> {
        let mut descendants = Vec::new();
        let mut queue = VecDeque::new();

        if let Some(node) = self.get_node(id) {
            for child_id in &node.children_ids {
                queue.push_back(child_id);
            }
        }

        while let Some(child_id) = queue.pop_front() {
            if let Some(child_node) = self.get_node(child_id) {
                descendants.push(child_node);
                for grand_child_id in &child_node.children_ids {
                    queue.push_back(grand_child_id);
                }
            }
        }

        descendants
    }

    /// Calculate the depth of a node in the hierarchy (root nodes have depth 0).
    pub fn depth_of(&self, id: &str) -> usize {
        self.get_ancestor_chain(id).len()
    }

    /// Return all root nodes (nodes without a parent).
    pub fn root_nodes(&self) -> Vec<&SceneNode> {
        self.root_ids
            .iter()
            .filter_map(|id| self.get_node(id))
            .collect()
    }

    // --- Orderings: Evaluation & Compositing ---

    /// Return the topological evaluation order for spatial transforms.
    ///
    /// Every parent node is guaranteed to appear BEFORE any of its children in the returned slice.
    /// Ties between independent branches are broken by original layer stack order.
    pub fn evaluation_order(&self) -> Result<Vec<&SceneNode>, SceneGraphError> {
        // In-degree calculation based on parent dependency (parent must come before child)
        // For node X: if X has a parent, in-degree is 1 (it depends on 1 parent).
        // A node with in-degree 0 has all dependencies satisfied and can be evaluated.
        let mut in_degree: HashMap<&str, usize> = HashMap::with_capacity(self.nodes.len());
        for (id, node) in &self.nodes {
            let deg = if node.has_parent() { 1 } else { 0 };
            in_degree.insert(id.as_str(), deg);
        }

        // Queue nodes with in-degree 0, ordered by stack_order to ensure determinism
        let mut ready: VecDeque<&str> = self
            .stack_order
            .iter()
            .filter(|id| in_degree.get(id.as_str()) == Some(&0))
            .map(|s| s.as_str())
            .collect();

        let mut order = Vec::with_capacity(self.nodes.len());

        while let Some(curr_id) = ready.pop_front() {
            let curr_node = self.nodes.get(curr_id).unwrap();
            order.push(curr_node);

            // For each child of curr_node, decrement its in-degree
            for child_id in &curr_node.children_ids {
                if let Some(deg) = in_degree.get_mut(child_id.as_str()) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        ready.push_back(child_id.as_str());
                    }
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(SceneGraphError::ParentCycleDetected {
                node_id: "unknown".to_string(),
                cycle: vec!["cyclic dependency prevented topological sort".to_string()],
            });
        }

        Ok(order)
    }

    /// Return all nodes in visual layer stack order (index 0 down to index N-1).
    pub fn layer_stack_order(&self) -> Vec<&SceneNode> {
        self.stack_order
            .iter()
            .filter_map(|id| self.get_node(id))
            .collect()
    }

    /// Return all nodes in painter's algorithm composite order (bottom layer rendered first,
    /// upper layers rendered over top).
    ///
    /// In After Effects conventions: layer at index 0 is topmost, layer at index (len-1) is bottom-most.
    /// Therefore, compositing renders from index (len-1) down to 0.
    pub fn composite_order(&self) -> Vec<&SceneNode> {
        self.stack_order
            .iter()
            .rev()
            .filter_map(|id| self.get_node(id))
            .collect()
    }

    /// Return all nodes that are active at the given timecode (in_point <= time < out_point),
    /// in visual layer stack order.
    pub fn active_nodes_at(&self, time: &TimeCode) -> Vec<&SceneNode> {
        self.layer_stack_order()
            .into_iter()
            .filter(|n| n.is_active_at(time))
            .collect()
    }

    /// Return all visible, active nodes at the given timecode in painter's composite order
    /// (bottom layer first, topmost layer last).
    pub fn render_order_at(&self, time: &TimeCode) -> Vec<&SceneNode> {
        self.composite_order()
            .into_iter()
            .filter(|n| n.is_visible_at(time))
            .collect()
    }

    /// Evaluate the spatial transforms for all nodes in the scene graph at the specified timecode
    /// using topological evaluation order.
    pub fn evaluate_transforms_at(
        &self,
        time: &TimeCode,
    ) -> Result<HashMap<String, EvaluatedTransform>, SceneGraphError> {
        TransformResolver::resolve_scene_graph_at(self, time)
    }

    /// Retrieve the evaluated spatial transform for a specific node in the scene graph at the specified timecode.
    pub fn get_evaluated_transform_at(
        &self,
        id: &str,
        time: &TimeCode,
    ) -> Result<EvaluatedTransform, SceneGraphError> {
        let transforms = self.evaluate_transforms_at(time)?;
        transforms
            .get(id)
            .copied()
            .ok_or_else(|| SceneGraphError::NodeNotFound(id.to_string()))
    }

    /// Evaluate the spatial transforms for all nodes in the scene graph using
    /// topological evaluation order at zero timecode.
    pub fn evaluate_transforms(&self) -> Result<HashMap<String, EvaluatedTransform>, SceneGraphError> {
        TransformResolver::resolve_scene_graph(self)
    }

    /// Retrieve the evaluated spatial transform for a specific node in the scene graph at zero timecode.
    pub fn get_evaluated_transform(&self, id: &str) -> Result<EvaluatedTransform, SceneGraphError> {
        let transforms = self.evaluate_transforms()?;
        transforms
            .get(id)
            .copied()
            .ok_or_else(|| SceneGraphError::NodeNotFound(id.to_string()))
    }
}
