use std::fmt;

/// Errors that can occur during scene graph construction, validation, and traversal.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneGraphError {
    ParentNotFound {
        node_id: String,
        parent_id: String,
    },
    SelfParenting(String),
    ParentCycleDetected {
        node_id: String,
        cycle: Vec<String>,
    },
    DuplicateNodeId(String),
    NodeNotFound(String),
    CompositionNotFound(String),
    InvalidDimensions {
        width: u32,
        height: u32,
    },
    InvalidFrameRate(f64),
    CircularNestedComposition {
        composition_id: String,
        cycle: Vec<String>,
    },
    MaxNestingDepthExceeded {
        depth: usize,
        max_depth: usize,
    },
}

impl fmt::Display for SceneGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParentNotFound { node_id, parent_id } => {
                write!(
                    f,
                    "scene node '{node_id}' references non-existent parent '{parent_id}'"
                )
            }
            Self::SelfParenting(id) => write!(f, "scene node '{id}' cannot be its own parent"),
            Self::ParentCycleDetected { node_id, cycle } => {
                write!(
                    f,
                    "parent cycle detected at node '{node_id}': {}",
                    cycle.join(" -> ")
                )
            }
            Self::DuplicateNodeId(id) => write!(f, "duplicate scene node ID: '{id}'"),
            Self::NodeNotFound(id) => write!(f, "scene node not found: '{id}'"),
            Self::CompositionNotFound(id) => write!(f, "composition not found: '{id}'"),
            Self::InvalidDimensions { width, height } => {
                write!(f, "invalid composition dimensions: {width}x{height}")
            }
            Self::InvalidFrameRate(fps) => write!(f, "invalid composition frame rate: {fps}"),
            Self::CircularNestedComposition { composition_id, cycle } => {
                write!(
                    f,
                    "circular nested composition detected for '{composition_id}': {}",
                    cycle.join(" -> ")
                )
            }
            Self::MaxNestingDepthExceeded { depth, max_depth } => {
                write!(
                    f,
                    "maximum nested composition depth exceeded: {depth} > {max_depth}"
                )
            }
        }
    }
}


impl std::error::Error for SceneGraphError {}
