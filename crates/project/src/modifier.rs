use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Mathematical operations supported by the Math node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

impl MathOp {
    pub const ALL: [Self; 5] = [
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Power,
    ];

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "−",
            Self::Multiply => "×",
            Self::Divide => "÷",
            Self::Power => "^",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Add => "Add (+)",
            Self::Subtract => "Subtract (−)",
            Self::Multiply => "Multiply (×)",
            Self::Divide => "Divide (÷)",
            Self::Power => "Power (^)",
        }
    }
}

/// Waveform types supported by the Wave node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveType {
    Sine,
    Triangle,
    Square,
}

impl WaveType {
    pub const ALL: [Self; 3] = [Self::Sine, Self::Triangle, Self::Square];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine Wave",
            Self::Triangle => "Triangle Wave",
            Self::Square => "Square Wave",
        }
    }
}

/// Node specific behavior, parameters, and socket configurations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    /// Reads the parent layer's normalized progress Factor ($0.0 \to 1.0$)
    /// and its inversion ($1.0 - \text{Factor}$).
    GetLayerFactor,

    /// Reads the incoming base value of the property being modified.
    BaseValueIn,

    /// Constant numeric value.
    Constant { value: f32 },

    /// Binary math operator (+, −, ×, ÷, ^).
    Math { op: MathOp, default_b: f32 },

    /// Linear interpolation between A and B driven by weight/factor.
    Lerp { default_a: f32, default_b: f32 },

    /// Clamps value between Min and Max.
    Clamp { min: f32, max: f32 },

    /// Remaps value from [InMin, InMax] to [OutMin, OutMax].
    Remap {
        in_min: f32,
        in_max: f32,
        out_min: f32,
        out_max: f32,
    },

    /// Smoothstep ease-in / ease-out S-curve.
    Smoothstep { edge0: f32, edge1: f32 },

    /// Procedural 1D gradient/value noise driven by Factor.
    Noise { frequency: f32, amplitude: f32 },

    /// Oscillation (Sine, Triangle, Square) across layer progression.
    Wave {
        wave_type: WaveType,
        frequency: f32,
        amplitude: f32,
        offset: f32,
    },

    /// Snaps/quantizes values into discrete stepped intervals.
    Stepped { steps: f32 },

    /// Live Driver Link node that reads another property on another (or the same) layer.
    DriverLink {
        driver_layer_id: String,
        driver_prop_path: String,
    },

    /// Terminal node that outputs the final value to the property.
    Output,
}

impl NodeKind {
    /// Display title for the node.
    pub fn title(&self) -> &'static str {
        match self {
            Self::GetLayerFactor => "Get Layer Factor",
            Self::BaseValueIn => "Base Value In",
            Self::Constant { .. } => "Constant",
            Self::DriverLink { .. } => "Driver Link",
            Self::Math { .. } => "Math",
            Self::Lerp { .. } => "Lerp",
            Self::Clamp { .. } => "Clamp",
            Self::Remap { .. } => "Remap",
            Self::Smoothstep { .. } => "Smoothstep",
            Self::Noise { .. } => "Noise",
            Self::Wave { .. } => "Wave / Sine",
            Self::Stepped { .. } => "Stepped",
            Self::Output => "Output",
        }
    }

    /// Input socket names for this node.
    pub fn input_sockets(&self) -> &'static [&'static str] {
        match self {
            Self::GetLayerFactor | Self::BaseValueIn | Self::Constant { .. } | Self::DriverLink { .. } => &[],
            Self::Math { .. } => &["a", "b"],
            Self::Lerp { .. } => &["a", "b", "weight"],
            Self::Clamp { .. } => &["val", "min", "max"],
            Self::Remap { .. } => &["val", "in_min", "in_max", "out_min", "out_max"],
            Self::Smoothstep { .. } => &["val", "edge0", "edge1"],
            Self::Noise { .. } => &["factor", "freq", "amp"],
            Self::Wave { .. } => &["factor", "freq", "amp"],
            Self::Stepped { .. } => &["val", "steps"],
            Self::Output => &["result"],
        }
    }

    /// Output socket names for this node.
    pub fn output_sockets(&self) -> &'static [&'static str] {
        match self {
            Self::GetLayerFactor => &["factor", "invert"],
            Self::BaseValueIn | Self::Constant { .. } | Self::DriverLink { .. } => &["value"],
            Self::Math { .. }
            | Self::Lerp { .. }
            | Self::Clamp { .. }
            | Self::Remap { .. }
            | Self::Smoothstep { .. }
            | Self::Noise { .. }
            | Self::Wave { .. }
            | Self::Stepped { .. } => &["result"],
            Self::Output => &[],
        }
    }
}

/// A node in the modifier graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModifierNode {
    pub id: String,
    pub title: String,
    pub pos_x: f32,
    pub pos_y: f32,
    pub kind: NodeKind,
}

impl ModifierNode {
    pub fn new(id: impl Into<String>, pos_x: f32, pos_y: f32, kind: NodeKind) -> Self {
        let title = kind.title().to_string();
        Self {
            id: id.into(),
            title,
            pos_x,
            pos_y,
            kind,
        }
    }
}

/// A directional wire connecting an output socket of one node to an input socket of another.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeConnection {
    pub from_node: String,
    pub from_socket: String,
    pub to_node: String,
    pub to_socket: String,
}

impl NodeConnection {
    pub fn new(
        from_node: impl Into<String>,
        from_socket: impl Into<String>,
        to_node: impl Into<String>,
        to_socket: impl Into<String>,
    ) -> Self {
        Self {
            from_node: from_node.into(),
            from_socket: from_socket.into(),
            to_node: to_node.into(),
            to_socket: to_socket.into(),
        }
    }
}

/// A graph of modifier nodes that evaluates on top of a property's base value
/// using the layer progression factor ($0.0 \to 1.0$).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModifierGraph {
    pub nodes: Vec<ModifierNode>,
    pub connections: Vec<NodeConnection>,
}

impl Default for ModifierGraph {
    fn default() -> Self {
        Self::default_passthrough()
    }
}

impl ModifierGraph {
    /// Create a standard default graph connecting BaseValueIn to Output.
    pub fn default_passthrough() -> Self {
        let base_node = ModifierNode::new("node_base", 40.0, 80.0, NodeKind::BaseValueIn);
        let factor_node = ModifierNode::new("node_factor", 40.0, 220.0, NodeKind::GetLayerFactor);
        let out_node = ModifierNode::new("node_output", 420.0, 140.0, NodeKind::Output);

        let conn = NodeConnection::new("node_base", "value", "node_output", "result");

        Self {
            nodes: vec![base_node, factor_node, out_node],
            connections: vec![conn],
        }
    }

    /// Find a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&ModifierNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Find a mutable node by ID.
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut ModifierNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: ModifierNode) {
        self.nodes.push(node);
    }

    /// Remove a node by ID and any connections referencing it.
    pub fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|n| n.id != id);
        self.connections
            .retain(|c| c.from_node != id && c.to_node != id);
    }

    /// Connect two sockets. Automatically replaces any existing connection to `to_node/to_socket`
    /// (since an input socket can only receive a single input wire).
    pub fn connect(
        &mut self,
        from_node: impl Into<String>,
        from_socket: impl Into<String>,
        to_node: impl Into<String>,
        to_socket: impl Into<String>,
    ) {
        let from_node = from_node.into();
        let from_socket = from_socket.into();
        let to_node = to_node.into();
        let to_socket = to_socket.into();

        // Disallow self-connections
        if from_node == to_node {
            return;
        }

        // Remove existing connection targeting this input socket
        self.connections
            .retain(|c| !(c.to_node == to_node && c.to_socket == to_socket));

        self.connections.push(NodeConnection::new(
            from_node,
            from_socket,
            to_node,
            to_socket,
        ));
    }

    /// Disconnect an input socket.
    pub fn disconnect_input(&mut self, to_node: &str, to_socket: &str) {
        self.connections
            .retain(|c| !(c.to_node == to_node && c.to_socket == to_socket));
    }

    /// Disconnect all connections matching a specific wire.
    pub fn disconnect(&mut self, conn: &NodeConnection) {
        self.connections.retain(|c| c != conn);
    }

    /// Evaluate the modifier graph given the target property's base value,
    /// the layer progression factor ($0.0 \to 1.0$), and a resolver callback for DriverLink nodes.
    pub fn evaluate_with_resolver(
        &self,
        base_value: f32,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
    ) -> f32 {
        let out_node = match self.nodes.iter().find(|n| matches!(n.kind, NodeKind::Output)) {
            Some(n) => n,
            None => return base_value,
        };

        let mut memo: HashMap<(String, String), f32> = HashMap::new();
        let mut call_stack: HashSet<String> = HashSet::new();

        self.resolve_input(
            &out_node.id,
            "result",
            base_value,
            factor,
            resolver,
            &mut memo,
            &mut call_stack,
        )
        .unwrap_or(base_value)
    }

    /// Evaluate the modifier graph given the target property's base value
    /// and the layer progression factor ($0.0 \to 1.0$).
    pub fn evaluate(&self, base_value: f32, factor: f32) -> f32 {
        self.evaluate_with_resolver(base_value, factor, &|_, _| 0.0)
    }

    /// Resolve an input socket by looking up an incoming connection or falling back to node defaults.
    #[allow(clippy::too_many_arguments)]
    fn resolve_input(
        &self,
        node_id: &str,
        socket: &str,
        base_value: f32,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
        memo: &mut HashMap<(String, String), f32>,
        call_stack: &mut HashSet<String>,
    ) -> Option<f32> {
        if let Some(conn) = self
            .connections
            .iter()
            .find(|c| c.to_node == node_id && c.to_socket == socket)
        {
            return Some(self.evaluate_node_output(
                &conn.from_node,
                &conn.from_socket,
                base_value,
                factor,
                resolver,
                memo,
                call_stack,
            ));
        }

        // No connection: fall back to node internal default parameter
        let node = self.get_node(node_id)?;
        match &node.kind {
            NodeKind::Math { default_b, .. } if socket == "b" => Some(*default_b),
            NodeKind::Math { .. } if socket == "a" => Some(base_value),
            NodeKind::Lerp { default_a, .. } if socket == "a" => Some(*default_a),
            NodeKind::Lerp { default_b, .. } if socket == "b" => Some(*default_b),
            NodeKind::Lerp { .. } if socket == "weight" => Some(factor),
            NodeKind::Clamp { min, .. } if socket == "min" => Some(*min),
            NodeKind::Clamp { max, .. } if socket == "max" => Some(*max),
            NodeKind::Remap { in_min, .. } if socket == "in_min" => Some(*in_min),
            NodeKind::Remap { in_max, .. } if socket == "in_max" => Some(*in_max),
            NodeKind::Remap { out_min, .. } if socket == "out_min" => Some(*out_min),
            NodeKind::Remap { out_max, .. } if socket == "out_max" => Some(*out_max),
            NodeKind::Smoothstep { edge0, .. } if socket == "edge0" => Some(*edge0),
            NodeKind::Smoothstep { edge1, .. } if socket == "edge1" => Some(*edge1),
            NodeKind::Noise { frequency, .. } if socket == "freq" => Some(*frequency),
            NodeKind::Noise { amplitude, .. } if socket == "amp" => Some(*amplitude),
            NodeKind::Noise { .. } if socket == "factor" => Some(factor),
            NodeKind::Wave { frequency, .. } if socket == "freq" => Some(*frequency),
            NodeKind::Wave { amplitude, .. } if socket == "amp" => Some(*amplitude),
            NodeKind::Wave { .. } if socket == "factor" => Some(factor),
            NodeKind::Stepped { steps } if socket == "steps" => Some(*steps),
            NodeKind::Stepped { .. } if socket == "val" => Some(factor),
            _ => None,
        }
    }

    /// Evaluate an output socket of a node.
    #[allow(clippy::too_many_arguments)]
    fn evaluate_node_output(
        &self,
        node_id: &str,
        socket: &str,
        base_value: f32,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
        memo: &mut HashMap<(String, String), f32>,
        call_stack: &mut HashSet<String>,
    ) -> f32 {
        let key = (node_id.to_string(), socket.to_string());
        if let Some(&val) = memo.get(&key) {
            return val;
        }

        // Cycle detection
        if !call_stack.insert(node_id.to_string()) {
            return 0.0;
        }

        let node = match self.get_node(node_id) {
            Some(n) => n,
            None => {
                call_stack.remove(node_id);
                return 0.0;
            }
        };

        let result = match &node.kind {
            NodeKind::GetLayerFactor => {
                if socket == "invert" {
                    1.0 - factor
                } else {
                    factor
                }
            }
            NodeKind::BaseValueIn => base_value,
            NodeKind::Constant { value } => *value,
            NodeKind::DriverLink { driver_layer_id, driver_prop_path } => {
                resolver(driver_layer_id, driver_prop_path)
            }
            NodeKind::Math { op, default_b } => {
                let a = self
                    .resolve_input(node_id, "a", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(base_value);
                let b = self
                    .resolve_input(node_id, "b", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*default_b);
                match op {
                    MathOp::Add => a + b,
                    MathOp::Subtract => a - b,
                    MathOp::Multiply => a * b,
                    MathOp::Divide => {
                        if b.abs() < 1e-6 {
                            a
                        } else {
                            a / b
                        }
                    }
                    MathOp::Power => a.powf(b),
                }
            }
            NodeKind::Lerp {
                default_a,
                default_b,
            } => {
                let a = self
                    .resolve_input(node_id, "a", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*default_a);
                let b = self
                    .resolve_input(node_id, "b", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*default_b);
                let w = self
                    .resolve_input(node_id, "weight", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(factor);
                a + (b - a) * w
            }
            NodeKind::Clamp { min, max } => {
                let val = self
                    .resolve_input(node_id, "val", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(base_value);
                let mn = self
                    .resolve_input(node_id, "min", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*min);
                let mx = self
                    .resolve_input(node_id, "max", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*max);
                val.clamp(mn.min(mx), mn.max(mx))
            }
            NodeKind::Remap {
                in_min,
                in_max,
                out_min,
                out_max,
            } => {
                let val = self
                    .resolve_input(node_id, "val", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(factor);
                let imin = self
                    .resolve_input(node_id, "in_min", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*in_min);
                let imax = self
                    .resolve_input(node_id, "in_max", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*in_max);
                let omin = self
                    .resolve_input(node_id, "out_min", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*out_min);
                let omax = self
                    .resolve_input(node_id, "out_max", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*out_max);
                let span_in = (imax - imin).abs().max(1e-6);
                let t = ((val - imin) / span_in).clamp(0.0, 1.0);
                omin + t * (omax - omin)
            }
            NodeKind::Smoothstep { edge0, edge1 } => {
                let val = self
                    .resolve_input(node_id, "val", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(factor);
                let e0 = self
                    .resolve_input(node_id, "edge0", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*edge0);
                let e1 = self
                    .resolve_input(node_id, "edge1", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*edge1);
                let span = (e1 - e0).abs().max(1e-6);
                let t = ((val - e0) / span).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
            NodeKind::Noise {
                frequency,
                amplitude,
            } => {
                let f = self
                    .resolve_input(node_id, "factor", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(factor);
                let freq = self
                    .resolve_input(node_id, "freq", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*frequency);
                let amp = self
                    .resolve_input(node_id, "amp", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*amplitude);
                let x = f * freq;
                let i0 = x.floor() as i64;
                let i1 = i0 + 1;
                let frac = x.fract();
                // Smooth interpolation for coherent noise
                let smooth = frac * frac * (3.0 - 2.0 * frac);
                let h0 = hash11(i0);
                let h1 = hash11(i1);
                let n = h0 + (h1 - h0) * smooth;
                n * amp
            }
            NodeKind::Wave {
                wave_type,
                frequency,
                amplitude,
                offset,
            } => {
                let f = self
                    .resolve_input(node_id, "factor", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(factor);
                let freq = self
                    .resolve_input(node_id, "freq", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*frequency);
                let amp = self
                    .resolve_input(node_id, "amp", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*amplitude);
                let phase = f * freq + *offset;
                match wave_type {
                    WaveType::Sine => (phase * std::f32::consts::TAU).sin() * amp,
                    WaveType::Triangle => {
                        let t = phase.fract();
                        let v = if t < 0.5 { t * 4.0 - 1.0 } else { 3.0 - t * 4.0 };
                        v * amp
                    }
                    WaveType::Square => {
                        let t = phase.fract();
                        let v = if t < 0.5 { 1.0 } else { -1.0 };
                        v * amp
                    }
                }
            }
            NodeKind::Stepped { steps } => {
                let val = self
                    .resolve_input(node_id, "val", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(factor);
                let s = self
                    .resolve_input(node_id, "steps", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(*steps)
                    .max(1.0);
                (val * s).floor() / s
            }
            NodeKind::Output => {
                self.resolve_input(node_id, "result", base_value, factor, resolver, memo, call_stack)
                    .unwrap_or(base_value)
            }
        };

        call_stack.remove(node_id);
        memo.insert(key, result);
        result
    }
}

/// 1D deterministic pseudo-random hash in range [-1.0, 1.0].
fn hash11(x: i64) -> f32 {
    let mut n = (x as u64).wrapping_mul(0x517cc1b727220a95);
    n ^= n >> 32;
    n = n.wrapping_mul(0x6c62272e07bb0142);
    n ^= n >> 32;
    let norm = (n as f64) / (u64::MAX as f64);
    (norm * 2.0 - 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_passthrough_evaluates_base_value() {
        let graph = ModifierGraph::default_passthrough();
        assert_eq!(graph.evaluate(150.0, 0.5), 150.0);
        assert_eq!(graph.evaluate(-25.0, 0.0), -25.0);
        assert_eq!(graph.evaluate(0.0, 1.0), 0.0);
    }

    #[test]
    fn test_layer_factor_and_invert() {
        let mut graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("factor", 0.0, 0.0, NodeKind::GetLayerFactor),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![NodeConnection::new("factor", "factor", "out", "result")],
        };
        assert_eq!(graph.evaluate(100.0, 0.25), 0.25);
        assert_eq!(graph.evaluate(100.0, 0.75), 0.75);

        // Switch to inverted output
        graph.connections[0].from_socket = "invert".to_string();
        assert_eq!(graph.evaluate(100.0, 0.25), 0.75);
        assert_eq!(graph.evaluate(100.0, 0.75), 0.25);
    }

    #[test]
    fn test_math_operations() {
        let ops = [
            (MathOp::Add, 10.0 + 5.0),
            (MathOp::Subtract, 10.0 - 5.0),
            (MathOp::Multiply, 10.0 * 5.0),
            (MathOp::Divide, 10.0 / 5.0),
            (MathOp::Power, 100000.0), // 10^5
        ];

        for (op, expected) in ops {
            let graph = ModifierGraph {
                nodes: vec![
                    ModifierNode::new("c1", 0.0, 0.0, NodeKind::Constant { value: 10.0 }),
                    ModifierNode::new("c2", 0.0, 50.0, NodeKind::Constant { value: 5.0 }),
                    ModifierNode::new("math", 100.0, 20.0, NodeKind::Math { op, default_b: 0.0 }),
                    ModifierNode::new("out", 200.0, 20.0, NodeKind::Output),
                ],
                connections: vec![
                    NodeConnection::new("c1", "value", "math", "a"),
                    NodeConnection::new("c2", "value", "math", "b"),
                    NodeConnection::new("math", "result", "out", "result"),
                ],
            };
            assert!((graph.evaluate(0.0, 0.5) - expected).abs() < 1e-4);
        }
    }

    #[test]
    fn test_lerp_with_factor() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("factor", 0.0, 0.0, NodeKind::GetLayerFactor),
                ModifierNode::new("lerp", 100.0, 0.0, NodeKind::Lerp { default_a: 10.0, default_b: 20.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("factor", "factor", "lerp", "weight"),
                NodeConnection::new("lerp", "result", "out", "result"),
            ],
        };
        assert_eq!(graph.evaluate(0.0, 0.0), 10.0);
        assert_eq!(graph.evaluate(0.0, 0.5), 15.0);
        assert_eq!(graph.evaluate(0.0, 1.0), 20.0);
    }

    #[test]
    fn test_clamp_and_remap() {
        let graph_clamp = ModifierGraph {
            nodes: vec![
                ModifierNode::new("base", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("clamp", 100.0, 0.0, NodeKind::Clamp { min: 0.0, max: 100.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("base", "value", "clamp", "val"),
                NodeConnection::new("clamp", "result", "out", "result"),
            ],
        };
        assert_eq!(graph_clamp.evaluate(-50.0, 0.0), 0.0);
        assert_eq!(graph_clamp.evaluate(50.0, 0.0), 50.0);
        assert_eq!(graph_clamp.evaluate(150.0, 0.0), 100.0);

        let graph_remap = ModifierGraph {
            nodes: vec![
                ModifierNode::new("base", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("remap", 100.0, 0.0, NodeKind::Remap { in_min: 0.0, in_max: 10.0, out_min: 100.0, out_max: 200.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("base", "value", "remap", "val"),
                NodeConnection::new("remap", "result", "out", "result"),
            ],
        };
        assert_eq!(graph_remap.evaluate(0.0, 0.0), 100.0);
        assert_eq!(graph_remap.evaluate(5.0, 0.0), 150.0);
        assert_eq!(graph_remap.evaluate(10.0, 0.0), 200.0);
    }

    #[test]
    fn test_smoothstep() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("factor", 0.0, 0.0, NodeKind::GetLayerFactor),
                ModifierNode::new("ss", 100.0, 0.0, NodeKind::Smoothstep { edge0: 0.0, edge1: 1.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("factor", "factor", "ss", "val"),
                NodeConnection::new("ss", "result", "out", "result"),
            ],
        };
        assert_eq!(graph.evaluate(0.0, 0.0), 0.0);
        assert_eq!(graph.evaluate(0.0, 0.5), 0.5);
        assert_eq!(graph.evaluate(0.0, 1.0), 1.0);
    }

    #[test]
    fn test_wave_and_noise() {
        let graph_wave = ModifierGraph {
            nodes: vec![
                ModifierNode::new("factor", 0.0, 0.0, NodeKind::GetLayerFactor),
                ModifierNode::new("wave", 100.0, 0.0, NodeKind::Wave {
                    wave_type: WaveType::Sine,
                    frequency: 1.0,
                    amplitude: 50.0,
                    offset: 0.0,
                }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("factor", "factor", "wave", "factor"),
                NodeConnection::new("wave", "result", "out", "result"),
            ],
        };
        assert!((graph_wave.evaluate(0.0, 0.0)).abs() < 1e-4);
        assert!((graph_wave.evaluate(0.0, 0.25) - 50.0).abs() < 1e-4);
        assert!((graph_wave.evaluate(0.0, 0.75) - (-50.0)).abs() < 1e-4);

        let graph_noise = ModifierGraph {
            nodes: vec![
                ModifierNode::new("factor", 0.0, 0.0, NodeKind::GetLayerFactor),
                ModifierNode::new("noise", 100.0, 0.0, NodeKind::Noise { frequency: 4.0, amplitude: 20.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("factor", "factor", "noise", "factor"),
                NodeConnection::new("noise", "result", "out", "result"),
            ],
        };
        let n0 = graph_noise.evaluate(0.0, 0.0);
        let n1 = graph_noise.evaluate(0.0, 0.5);
        assert!((-20.0..=20.0).contains(&n0));
        assert!((-20.0..=20.0).contains(&n1));
    }

    #[test]
    fn test_stepped_quantization() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("factor", 0.0, 0.0, NodeKind::GetLayerFactor),
                ModifierNode::new("step", 100.0, 0.0, NodeKind::Stepped { steps: 4.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("factor", "factor", "step", "val"),
                NodeConnection::new("step", "result", "out", "result"),
            ],
        };
        assert_eq!(graph.evaluate(0.0, 0.1), 0.0);
        assert_eq!(graph.evaluate(0.0, 0.26), 0.25);
        assert_eq!(graph.evaluate(0.0, 0.51), 0.5);
        assert_eq!(graph.evaluate(0.0, 0.76), 0.75);
    }

    #[test]
    fn test_cycle_protection() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("n1", 0.0, 0.0, NodeKind::Math { op: MathOp::Add, default_b: 1.0 }),
                ModifierNode::new("n2", 100.0, 0.0, NodeKind::Math { op: MathOp::Add, default_b: 1.0 }),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("n1", "result", "n2", "a"),
                NodeConnection::new("n2", "result", "n1", "a"),
                NodeConnection::new("n2", "result", "out", "result"),
            ],
        };
        // Circular loop must not hang or overflow the stack
        let _ = graph.evaluate(10.0, 0.5);
    }

    #[test]
    fn test_driver_link_evaluation() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new(
                    "driver_1",
                    0.0,
                    0.0,
                    NodeKind::DriverLink {
                        driver_layer_id: "layer_master".to_string(),
                        driver_prop_path: "transform.rotation".to_string(),
                    },
                ),
                ModifierNode::new(
                    "math_double",
                    150.0,
                    0.0,
                    NodeKind::Math {
                        op: MathOp::Multiply,
                        default_b: 2.0,
                    },
                ),
                ModifierNode::new("out", 300.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("driver_1", "value", "math_double", "a"),
                NodeConnection::new("math_double", "result", "out", "result"),
            ],
        };

        // Driver resolves to 45.0
        let res = graph.evaluate_with_resolver(0.0, 0.0, &|lid, prop| {
            if lid == "layer_master" && prop == "transform.rotation" {
                45.0
            } else {
                0.0
            }
        });
        assert_eq!(res, 90.0);
    }
}
