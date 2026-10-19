use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::StockPlugin;

/// Value type of a Constant node (drives its scalar/vector/color storage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConstantMode {
    #[default]
    Float,
    Int,
    Color,
    Vec2,
    Vec3,
}

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

/// Pointwise blend modes of the Node Graph's Blend node. Applied per
/// channel; a `weight` input (wired or default) mixes the mode result back
/// toward side A, so unwired weight (factor 0 in effect graphs) keeps A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GraphBlendMode {
    #[default]
    Linear,
    Screen,
    Multiply,
    Add,
    Subtract,
    Difference,
    Overlay,
}

impl GraphBlendMode {
    pub const ALL: [Self; 7] = [
        Self::Linear,
        Self::Screen,
        Self::Multiply,
        Self::Add,
        Self::Subtract,
        Self::Difference,
        Self::Overlay,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Screen => "Screen",
            Self::Multiply => "Multiply",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Difference => "Difference",
            Self::Overlay => "Overlay",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }

    /// The mode's raw a/b mix; weight lerp happens in the node eval.
    fn mix(self, a: f32, b: f32) -> f32 {
        match self {
            Self::Linear => a + (b - a),
            Self::Screen => 1.0 - (1.0 - a) * (1.0 - b),
            Self::Multiply => a * b,
            Self::Add => a + b,
            Self::Subtract => a - b,
            Self::Difference => (a - b).abs(),
            Self::Overlay => {
                if a < 0.5 {
                    2.0 * a * b
                } else {
                    1.0 - 2.0 * (1.0 - a) * (1.0 - b)
                }
            }
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

    /// Typed constant value: scalar (Float/Int) fans across channels;
    /// vector/color modes hold per-channel components delivered through the
    /// single combined `value` socket. Legacy `value` field = first channel.
    Constant {
        #[serde(default)]
        mode: ConstantMode,
        value: f32,
        #[serde(default)]
        g: f32,
        #[serde(default)]
        b: f32,
        #[serde(default)]
        a: f32,
    },

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

    /// Gather scalar channels into one vector value (dims 2 = 2D, 3 = 3D).
    /// Unwired channels pass the matching base component through.
    MakeVector { dims: u8 },

    /// Unpack an incoming vector bus (BaseValueIn / MakeVector) into scalar channels.
    BreakVector { dims: u8 },

    /// Gather R/G/B/A channels into one color value. Unwired channels pass
    /// the matching base color component through.
    MakeColor,

    /// Unpack an incoming color bus (BaseValueIn / MakeColor) into scalar channels.
    BreakColor,

    /// Node Graph Blend: pointwise per-channel mix of two values (A and B)
    /// by a `weight`. A unwired = base value, B unwired = stored default,
    /// weight unwired = factor (0 inside effect graphs, so unwired blends
    /// pass A through).
    Blend {
        #[serde(default)]
        mode: GraphBlendMode,
        #[serde(default)]
        default_b: f32,
    },

    /// Node Graph stock plug-in (pointwise only): a whole per-pixel color
    /// effect as a single node. Input is the color bus; params are raw
    /// descriptor-order floats. Spatial plug-ins (Blur, Glow, ...) cannot
    /// be nodes.
    StockOp {
        plugin: StockPlugin,
        #[serde(default)]
        params: Vec<f32>,
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
            Self::MakeVector { dims: 2 } => "Make 2D",
            Self::MakeVector { dims: 3 } => "Make 3D",
            Self::MakeVector { .. } => "Make Vector",
            Self::BreakVector { dims: 2 } => "Break 2D",
            Self::BreakVector { dims: 3 } => "Break 3D",
            Self::BreakVector { .. } => "Break Vector",
            Self::MakeColor => "Make Color",
            Self::BreakColor => "Break Color",
            Self::Blend { .. } => "Blend",
            Self::StockOp { plugin, .. } => plugin.descriptor().label,
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
            Self::MakeVector { dims: 3 } => &["x", "y", "z"],
            Self::MakeVector { .. } => &["x", "y"],
            Self::BreakVector { .. } => &["in"],
            Self::MakeColor => &["r", "g", "b", "a"],
            Self::BreakColor => &["in"],
            Self::Blend { .. } => &["a", "b", "weight"],
            Self::StockOp { .. } => &["in"],
            Self::Output => &["result"],
        }
    }

    /// Output socket names for this node.
    pub fn output_sockets(&self) -> &'static [&'static str] {
        match self {
            Self::GetLayerFactor => &["factor", "invert"],
            Self::Constant { .. } | Self::DriverLink { .. } => &["value"],
            Self::Math { .. }
            | Self::Lerp { .. }
            | Self::Clamp { .. }
            | Self::Remap { .. }
            | Self::Smoothstep { .. }
            | Self::Noise { .. }
            | Self::Wave { .. }
            | Self::Stepped { .. } => &["result"],
            Self::BaseValueIn => &["value"],
            Self::MakeVector { .. } | Self::MakeColor => &["value"],
            Self::BreakVector { dims: 3 } => &["x", "y", "z"],
            Self::BreakVector { .. } => &["x", "y"],
            Self::BreakColor => &["r", "g", "b", "a"],
            Self::Blend { .. } | Self::StockOp { .. } => &["value"],
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

/// Base value unpacked into its components for vector/color modifier graphs.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BaseComponents {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl BaseComponents {
    /// Fan a scalar binding: every component mirrors the base value.
    pub fn fan(base: f32) -> Self {
        Self { x: base, y: base, z: base, w: base }
    }

    /// Read one signed channel by socket name ("value"/"x"/"r", "y"/"g",
    /// "z"/"b", "a"). Unknown names fall through to alpha.
    pub fn comp_value(&self, chan: &str) -> f32 {
        match chan {
            "value" | "x" | "r" => self.x,
            "y" | "g" => self.y,
            "z" | "b" => self.z,
            _ => self.w,
        }
    }
}

/// Constant components per mode: scalars fan across every channel so legacy
/// scalar graphs behave unchanged; vector/color modes hold real components.
fn constant_comp(mode: ConstantMode, value: f32, g: f32, b: f32, a: f32) -> BaseComponents {
    match mode {
        ConstantMode::Float | ConstantMode::Int => BaseComponents::fan(value),
        ConstantMode::Color => BaseComponents { x: value, y: g, z: b, w: a },
        ConstantMode::Vec2 => BaseComponents { x: value, y: g, z: 0.0, w: 1.0 },
        ConstantMode::Vec3 => BaseComponents { x: value, y: g, z: b, w: 1.0 },
    }
}

/// Internal evaluation context threaded through the recursive resolver.
/// `channel` is the session's query channel ("value" for scalar bindings,
/// or "x"/"y"/"z"/"r"/"g"/"b"/"a" when the caller extracts one component of
/// a multi-dimensional base).
struct EvalCtx<'a> {
    base: f32,
    comp: &'a BaseComponents,
    factor: f32,
    resolver: &'a dyn Fn(&str, &str) -> f32,
    channel: &'static str,
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

        let comp = BaseComponents::fan(base_value);
        let ctx = EvalCtx { base: base_value, comp: &comp, factor, resolver, channel: "value" };
        let mut memo: HashMap<(String, String), f32> = HashMap::new();
        let mut call_stack: HashSet<String> = HashSet::new();

        self.resolve_input(
            &out_node.id,
            "result",
            &ctx,
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

    /// Evaluate one component of a multi-dimensional binding by re-running
    /// the whole Output `result` path under that channel's query (make nodes
    /// route by `ctx.channel`; break nodes expose per-channel sockets).
    fn evaluate_channel(
        &self,
        out_id: &str,
        query: &'static str,
        base: f32,
        comp: &BaseComponents,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
    ) -> f32 {
        let ctx = EvalCtx { base, comp, factor, resolver, channel: query };
        let mut memo: HashMap<(String, String), f32> = HashMap::new();
        let mut call_stack: HashSet<String> = HashSet::new();
        self.resolve_input(out_id, "result", &ctx, &mut memo, &mut call_stack)
            .unwrap_or(base)
    }

    /// Evaluate a 2D vector binding; each axis re-runs the combined
    /// `Output.result` path (legacy scalar graphs fan per channel as before).
    pub fn evaluate_vec2(
        &self,
        base_x: f32,
        base_y: f32,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
    ) -> (f32, f32) {
        let out = match self.nodes.iter().find(|n| matches!(n.kind, NodeKind::Output)) {
            Some(n) => n.id.clone(),
            None => return (base_x, base_y),
        };
        let comp = BaseComponents { x: base_x, y: base_y, z: 0.0, w: 1.0 };
        let x = self.evaluate_channel(&out, "x", base_x, &comp, factor, resolver);
        let y = self.evaluate_channel(&out, "y", base_y, &comp, factor, resolver);
        (x, y)
    }

    /// Evaluate a 3D vector binding (x/y/z re-runs of the result path).
    pub fn evaluate_vec3(
        &self,
        base_x: f32,
        base_y: f32,
        base_z: f32,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
    ) -> (f32, f32, f32) {
        let out = match self.nodes.iter().find(|n| matches!(n.kind, NodeKind::Output)) {
            Some(n) => n.id.clone(),
            None => return (base_x, base_y, base_z),
        };
        let comp = BaseComponents { x: base_x, y: base_y, z: base_z, w: 1.0 };
        let x = self.evaluate_channel(&out, "x", base_x, &comp, factor, resolver);
        let y = self.evaluate_channel(&out, "y", base_y, &comp, factor, resolver);
        let z = self.evaluate_channel(&out, "z", base_z, &comp, factor, resolver);
        (x, y, z)
    }

    /// Evaluate a color binding; each RGBA channel re-runs the combined
    /// `Output.result` path (legacy scalar graphs fan per channel as before).
    pub fn evaluate_color(
        &self,
        base_r: f32,
        base_g: f32,
        base_b: f32,
        base_a: f32,
        factor: f32,
        resolver: &dyn Fn(&str, &str) -> f32,
    ) -> [f32; 4] {
        let out = match self.nodes.iter().find(|n| matches!(n.kind, NodeKind::Output)) {
            Some(n) => n.id.clone(),
            None => return [base_r, base_g, base_b, base_a],
        };
        let comp = BaseComponents { x: base_r, y: base_g, z: base_b, w: base_a };
        [
            self.evaluate_channel(&out, "r", base_r, &comp, factor, resolver),
            self.evaluate_channel(&out, "g", base_g, &comp, factor, resolver),
            self.evaluate_channel(&out, "b", base_b, &comp, factor, resolver),
            self.evaluate_channel(&out, "a", base_a, &comp, factor, resolver),
        ]
    }

    /// Whether the graph is a color-clean chain suitable for the WGSL GPU
    /// twin: every node reachable backwards from Output must be Content
    /// (BaseValueIn), Constant, pointwise StockOp, or Blend. Unwired Blend
    /// inputs stay twin-safe (B → stored default, weight → factor, A →
    /// the pixel itself); the renderer additionally caps uniform budget.
    pub fn is_color_clean(&self) -> bool {
        let Some(out) = self.nodes.iter().find(|n| matches!(n.kind, NodeKind::Output)) else {
            return false;
        };
        let mut stack = vec![out.id.clone()];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(node) = self.get_node(&id) else {
                return false;
            };
            let ok = match &node.kind {
                NodeKind::Output | NodeKind::BaseValueIn | NodeKind::Constant { .. } | NodeKind::Blend { .. } => true,
                NodeKind::StockOp { plugin, .. } => plugin.is_pointwise(),
                _ => false,
            };
            if !ok {
                return false;
            }
            for conn in &self.connections {
                if conn.to_node == id {
                    stack.push(conn.from_node.clone());
                }
            }
        }
        true
    }

    /// Resolve an input socket by looking up an incoming connection or falling back to node defaults.
    fn resolve_input(
        &self,
        node_id: &str,
        socket: &str,
        ctx: &EvalCtx,
        memo: &mut HashMap<(String, String), f32>,
        call_stack: &mut HashSet<String>,
    ) -> Option<f32> {
        let base_value = ctx.base;
        let factor = ctx.factor;
        if let Some(conn) = self
            .connections
            .iter()
            .find(|c| c.to_node == node_id && c.to_socket == socket)
        {
            return Some(self.evaluate_node_output(
                &conn.from_node,
                &conn.from_socket,
                ctx, memo, call_stack,
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
            NodeKind::Blend { default_b, .. } if socket == "b" => Some(*default_b),
            NodeKind::Blend { .. } if socket == "weight" => Some(factor),
            // Unwired source color: pass the matching base component through
            // (comp mirrors the session channel; "value" sessions fan).
            NodeKind::StockOp { .. } if socket == "in" => Some(ctx.comp.comp_value(ctx.channel)),
            // Make nodes: unwired channels pass the matching base component
            // through (gathers stay identity when nothing is rewired).
            NodeKind::MakeVector { .. } | NodeKind::MakeColor => match socket {
                "x" | "r" => Some(ctx.comp.x),
                "y" | "g" => Some(ctx.comp.y),
                "z" | "b" => Some(ctx.comp.z),
                "a" => Some(ctx.comp.w),
                _ => Some(base_value),
            },
            _ => None,
        }
    }

    /// Evaluate an output socket of a node.
    fn evaluate_node_output(
        &self,
        node_id: &str,
        socket: &str,
        ctx: &EvalCtx,
        memo: &mut HashMap<(String, String), f32>,
        call_stack: &mut HashSet<String>,
    ) -> f32 {
        let base_value = ctx.base;
        let factor = ctx.factor;
        let resolver = ctx.resolver;
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
            NodeKind::MakeVector { .. } | NodeKind::MakeColor => {
                // Single combined output: the make echoes the input channel
                // the session is currently querying ("value" -> ctx.base).
                let chan = if socket == "value" { ctx.channel } else { socket };
                self.resolve_input(node_id, chan, ctx, memo, call_stack)
                    .unwrap_or(if chan == "value" { base_value } else { ctx.comp.comp_value(chan) })
            }
            NodeKind::BreakVector { .. } | NodeKind::BreakColor => {
                // Follow the incoming wire: a make source is asked for the
                // same component; a channel-output source (break bus) is read
                // directly; a scalar source replicates across channels.
                if let Some(conn) = self
                    .connections
                    .iter()
                    .find(|c| c.to_node == node_id && c.to_socket == "in")
                {
                    let src = conn.from_node.clone();
                    let src_kind = self.get_node(&src).map(|n| n.kind.clone());
                    let query = if socket == "value" { ctx.channel } else { socket };
                    match &src_kind {
                        Some(NodeKind::MakeVector { .. }) | Some(NodeKind::MakeColor) => {
                            self.evaluate_node_output(&src, query, ctx, memo, call_stack)
                        }
                        Some(k) if k.output_sockets().contains(&query) => {
                            self.evaluate_node_output(&src, query, ctx, memo, call_stack)
                        }
                        _ => self.evaluate_node_output(&src, &conn.from_socket, ctx, memo, call_stack),
                    }
                } else {
                    base_value
                }
            }
            NodeKind::Constant { mode, value, g, b, a } => {
                let chan = if socket == "value" { ctx.channel } else { socket };
                constant_comp(*mode, *value, *g, *b, *a).comp_value(chan)
            }
            NodeKind::DriverLink { driver_layer_id, driver_prop_path } => {
                resolver(driver_layer_id, driver_prop_path)
            }
            NodeKind::Math { op, default_b } => {
                let a = self
                    .resolve_input(node_id, "a", ctx, memo, call_stack,)
                    .unwrap_or(base_value);
                let b = self
                    .resolve_input(node_id, "b", ctx, memo, call_stack,)
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
                    .resolve_input(node_id, "a", ctx, memo, call_stack,)
                    .unwrap_or(*default_a);
                let b = self
                    .resolve_input(node_id, "b", ctx, memo, call_stack,)
                    .unwrap_or(*default_b);
                let w = self
                    .resolve_input(node_id, "weight", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                a + (b - a) * w
            }
            NodeKind::Clamp { min, max } => {
                let val = self
                    .resolve_input(node_id, "val", ctx, memo, call_stack,)
                    .unwrap_or(base_value);
                let mn = self
                    .resolve_input(node_id, "min", ctx, memo, call_stack,)
                    .unwrap_or(*min);
                let mx = self
                    .resolve_input(node_id, "max", ctx, memo, call_stack,)
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
                    .resolve_input(node_id, "val", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                let imin = self
                    .resolve_input(node_id, "in_min", ctx, memo, call_stack,)
                    .unwrap_or(*in_min);
                let imax = self
                    .resolve_input(node_id, "in_max", ctx, memo, call_stack,)
                    .unwrap_or(*in_max);
                let omin = self
                    .resolve_input(node_id, "out_min", ctx, memo, call_stack,)
                    .unwrap_or(*out_min);
                let omax = self
                    .resolve_input(node_id, "out_max", ctx, memo, call_stack,)
                    .unwrap_or(*out_max);
                let span_in = (imax - imin).abs().max(1e-6);
                let t = ((val - imin) / span_in).clamp(0.0, 1.0);
                omin + t * (omax - omin)
            }
            NodeKind::Smoothstep { edge0, edge1 } => {
                let val = self
                    .resolve_input(node_id, "val", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                let e0 = self
                    .resolve_input(node_id, "edge0", ctx, memo, call_stack,)
                    .unwrap_or(*edge0);
                let e1 = self
                    .resolve_input(node_id, "edge1", ctx, memo, call_stack,)
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
                    .resolve_input(node_id, "factor", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                let freq = self
                    .resolve_input(node_id, "freq", ctx, memo, call_stack,)
                    .unwrap_or(*frequency);
                let amp = self
                    .resolve_input(node_id, "amp", ctx, memo, call_stack,)
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
                    .resolve_input(node_id, "factor", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                let freq = self
                    .resolve_input(node_id, "freq", ctx, memo, call_stack,)
                    .unwrap_or(*frequency);
                let amp = self
                    .resolve_input(node_id, "amp", ctx, memo, call_stack,)
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
                    .resolve_input(node_id, "val", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                let s = self
                    .resolve_input(node_id, "steps", ctx, memo, call_stack,)
                    .unwrap_or(*steps)
                    .max(1.0);
                (val * s).floor() / s
            }
            NodeKind::Blend { mode, .. } => {
                let a = self
                    .resolve_input(node_id, "a", ctx, memo, call_stack,)
                    .unwrap_or(base_value);
                let b = self
                    .resolve_input(node_id, "b", ctx, memo, call_stack,)
                    .unwrap_or(0.0);
                let w = self
                    .resolve_input(node_id, "weight", ctx, memo, call_stack,)
                    .unwrap_or(factor);
                let mixed = mode.mix(a, b);
                a + (mixed - a) * w
            }
            NodeKind::StockOp { plugin, params } => {
                // A stock node is a color bus: gather its source per RGBA
                // channel by re-resolving the `in` wire under per-channel
                // sub-contexts (independent memos, so cross-channel make/
                // break wiring stays correct), run the shared pointwise
                // kernel, and answer this session's channel component.
                let chan = if socket == "value" { ctx.channel } else { socket };
                let mut src = [0.0f32; 4];
                for (i, ch) in ["r", "g", "b", "a"].iter().enumerate() {
                    let ch = *ch;
                    let sub = EvalCtx {
                        base: ctx.comp.comp_value(ch),
                        comp: ctx.comp,
                        factor: ctx.factor,
                        resolver: ctx.resolver,
                        channel: ch,
                    };                    let mut sub_memo = HashMap::new();
                    let mut sub_stack = HashSet::new();
                    src[i] = self
                        .resolve_input(node_id, "in", &sub, &mut sub_memo, &mut sub_stack)
                        .unwrap_or(sub.base);
                }
                let out = crate::stock::process_color_stock(
                    *plugin,
                    params,
                    crate::Color::rgba(src[0], src[1], src[2], src[3]),
                );
                match chan {
                    "r" => out.r,
                    "g" => out.g,
                    "b" => out.b,
                    _ => out.a,
                }
            }
            NodeKind::Output => {
                self.resolve_input(node_id, "result", ctx, memo, call_stack,)
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
                    ModifierNode::new("c1", 0.0, 0.0, NodeKind::Constant { mode: ConstantMode::Float, value: 10.0, g: 0.0, b: 0.0, a: 1.0 }),
                    ModifierNode::new("c2", 0.0, 50.0, NodeKind::Constant { mode: ConstantMode::Float, value: 5.0, g: 0.0, b: 0.0, a: 1.0 }),
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

    /// Make 2D fed by per-channel math; unwired Y passes the base through.
    #[test]
    fn test_make_vector_per_channel() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("addx", 40.0, 0.0, NodeKind::Math { op: MathOp::Add, default_b: 0.0 }),
                ModifierNode::new("addc", 40.0, 60.0, NodeKind::Constant { mode: ConstantMode::Float, value: 25.0, g: 0.0, b: 0.0, a: 1.0 }),
                ModifierNode::new("mk", 120.0, 0.0, NodeKind::MakeVector { dims: 2 }),
                ModifierNode::new("out", 220.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "addx", "a"),
                NodeConnection::new("addc", "value", "addx", "b"),
                NodeConnection::new("addx", "result", "mk", "x"),
                NodeConnection::new("mk", "value", "out", "result"),
            ],
        };
        // x pass: make echoes its wired x input (+25); y pass: make's y
        // input unwired → base y passthrough.
        let (x, y) = graph.evaluate_vec2(100.0, 200.0, 0.5, &|_, _| 0.0);
        assert!((x - 125.0).abs() < 1e-5, "x got {x}");
        assert!((y - 200.0).abs() < 1e-5, "y got {y}");

        // Rewire make.y via a subtract: only y is changed, x passes through.
        let mut g2 = graph.clone();
        g2.connections.retain(|c| !(c.to_node == "mk" && c.to_socket == "x"));
        g2.nodes.push(ModifierNode::new("c10", 60.0, 120.0, NodeKind::Constant { mode: ConstantMode::Float, value: 10.0, g: 0.0, b: 0.0, a: 1.0 }));
        g2.nodes.push(ModifierNode::new("suby", 60.0, 180.0, NodeKind::Math { op: MathOp::Subtract, default_b: 0.0 }));
        g2.connect("c10", "value", "suby", "b");
        g2.connect("suby", "result", "mk", "y");
        let (x, y) = g2.evaluate_vec2(100.0, 200.0, 0.5, &|_, _| 0.0);
        assert!((x - 100.0).abs() < 1e-5, "unwired make channel defaults to comp.x, got {x}");
        assert!((y - 190.0).abs() < 1e-5, "wired make.y 200-10, got {y}");
    }

    /// Break unpacks the base bus; make re-gathers into one combined output.
    #[test]
    fn test_break_vector_unpacks_base() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("brk", 60.0, 0.0, NodeKind::BreakVector { dims: 2 }),
                ModifierNode::new("mkp", 140.0, 0.0, NodeKind::MakeVector { dims: 2 }),
                ModifierNode::new("out", 220.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "brk", "in"),
                NodeConnection::new("brk", "x", "mkp", "x"),
                NodeConnection::new("brk", "y", "mkp", "y"),
                NodeConnection::new("mkp", "value", "out", "result"),
            ],
        };
        let (x, y) = graph.evaluate_vec2(100.0, 200.0, 0.5, &|_, _| 0.0);
        assert!((x - 100.0).abs() < 1e-5, "x got {x}");
        assert!((y - 200.0).abs() < 1e-5, "y got {y}");
    }

    /// Make Color drives channels independently (classic black +50 red),
    /// delivered to the Output through its single combined socket.
    #[test]
    fn test_make_color_per_channel() {
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("cc", 0.0, 0.0, NodeKind::Constant { mode: ConstantMode::Float, value: 0.5, g: 0.0, b: 0.0, a: 1.0 }),
                ModifierNode::new("mk", 100.0, 0.0, NodeKind::MakeColor),
                ModifierNode::new("out", 220.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("cc", "value", "mk", "r"),
                NodeConnection::new("mk", "value", "out", "result"),
            ],
        };
        let fan = graph.evaluate_color(0.0, 0.0, 0.0, 1.0, 0.5, &|_, _| 0.0);
        assert!((fan[0] - 0.5).abs() < 1e-5, "r got {}", fan[0]);
        assert!((fan[1] - 0.0).abs() < 1e-5, "g got {}", fan[1]);
        assert!((fan[2] - 0.0).abs() < 1e-5, "b got {}", fan[2]);
        assert!((fan[3] - 1.0).abs() < 1e-5, "a got {}", fan[3]);
    }

    /// Typed constants: a Vec2/Vec3/Color constant delivered straight to the
    /// Output carries its per-channel components; Int behaves like Float.
    #[test]
    fn test_typed_constant_modes() {
        let graph_of = |kind: NodeKind| ModifierGraph {
            nodes: vec![
                ModifierNode::new("c", 0.0, 0.0, kind),
                ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
            ],
            connections: vec![NodeConnection::new("c", "value", "out", "result")],
        };

        let v2 = graph_of(NodeKind::Constant {
            mode: ConstantMode::Vec2,
            value: 10.0,
            g: 20.0,
            b: 0.0,
            a: 1.0,
        });
        let (x, y) = v2.evaluate_vec2(100.0, 200.0, 0.5, &|_, _| 0.0);
        assert!((x - 10.0).abs() < 1e-5, "vec2 x got {x}");
        assert!((y - 20.0).abs() < 1e-5, "vec2 y got {y}");

        let v3 = graph_of(NodeKind::Constant {
            mode: ConstantMode::Vec3,
            value: 1.0,
            g: 2.0,
            b: 3.0,
            a: 1.0,
        });
        let (x, y, z) = v3.evaluate_vec3(100.0, 200.0, 300.0, 0.5, &|_, _| 0.0);
        assert!((x - 1.0).abs() < 1e-5 && (y - 2.0).abs() < 1e-5 && (z - 3.0).abs() < 1e-5);

        let col = graph_of(NodeKind::Constant {
            mode: ConstantMode::Color,
            value: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        let fan = col.evaluate_color(0.0, 0.0, 0.0, 1.0, 0.5, &|_, _| 0.0);
        assert!((fan[0] - 1.0).abs() < 1e-5 && (fan[1] - 0.0).abs() < 1e-5 && (fan[2] - 0.0).abs() < 1e-5 && (fan[3] - 1.0).abs() < 1e-5);

        // Int mode evaluates like Float.
        let int = graph_of(NodeKind::Constant {
            mode: ConstantMode::Int,
            value: 7.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        assert!((int.evaluate(0.0, 0.5) - 7.0).abs() < 1e-5);
        assert!((int.evaluate_color(0.9, 0.9, 0.9, 0.9, 0.5, &|_, _| 0.0)[0] - 7.0).abs() < 1e-5);
    }

    /// General nodes (Math, Lerp, Clamp…) accept every value format: their
    /// scalar sockets evaluate under the session's per-channel query, so
    /// vector/color flows just work when wired through Make's combined value.
    #[test]
    fn test_general_nodes_accept_all_value_formats() {
        // Vec2 through Math: x = base.x * 2 (Multiply), y = base.y + 50 (Add).
        let vec_math = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("m2", 50.0, 0.0, NodeKind::Math { op: MathOp::Multiply, default_b: 2.0 }),
                ModifierNode::new("a5", 50.0, 60.0, NodeKind::Math { op: MathOp::Add, default_b: 50.0 }),
                ModifierNode::new("mk", 140.0, 30.0, NodeKind::MakeVector { dims: 2 }),
                ModifierNode::new("out", 240.0, 30.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "m2", "a"),
                NodeConnection::new("m2", "result", "mk", "x"),
                NodeConnection::new("in", "value", "a5", "a"),
                NodeConnection::new("a5", "result", "mk", "y"),
                NodeConnection::new("mk", "value", "out", "result"),
            ],
        };
        let (x, y) = vec_math.evaluate_vec2(100.0, 10.0, 0.5, &|_, _| 0.0);
        assert!((x - 200.0).abs() < 1e-5, "x 100*2, got {x}");
        assert!((y - 60.0).abs() < 1e-5, "y 10+50, got {y}");

        // Color through Lerp: mix base color with a red Make at weight 0.25.
        let color_lerp = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("red", 60.0, 60.0, NodeKind::MakeColor),
                ModifierNode::new("one", 60.0, 120.0, NodeKind::Constant {
                    mode: ConstantMode::Float,
                    value: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }),
                ModifierNode::new("w", 60.0, 180.0, NodeKind::Constant {
                    mode: ConstantMode::Float,
                    value: 0.25,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }),
                ModifierNode::new("lp", 140.0, 30.0, NodeKind::Lerp { default_a: 0.0, default_b: 1.0 }),
                ModifierNode::new("out", 240.0, 30.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "lp", "a"),
                NodeConnection::new("red", "value", "lp", "b"),
                NodeConnection::new("one", "value", "red", "r"),
                NodeConnection::new("w", "value", "lp", "weight"),
                NodeConnection::new("lp", "result", "out", "result"),
            ],
        };
        // make.x wired: r = lerp(0, 1, 0.25) = 0.25; g/b/a channels unwired
        // on the make → pass through the per-channel base.
        let fan = color_lerp.evaluate_color(0.0, 0.0, 0.0, 1.0, 0.5, &|_, _| 0.0);
        assert!((fan[0] - 0.25).abs() < 1e-5, "r lerp 0.25 → 0.25, got {}", fan[0]);
        assert!((fan[1] - 0.0).abs() < 1e-5, "g passthrough");
        assert!((fan[2] - 0.0).abs() < 1e-5, "b passthrough");
        assert!((fan[3] - 1.0).abs() < 1e-5, "a passthrough");

        // Scalar Clamp still works with a color value flowing per channel.
        let clamped = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("cl", 60.0, 0.0, NodeKind::Clamp { min: 0.0, max: 0.5 }),
                ModifierNode::new("out", 160.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "cl", "val"),
                NodeConnection::new("cl", "result", "out", "result"),
            ],
        };
        let fan = clamped.evaluate_color(1.0, 0.75, 0.5, 1.0, 0.5, &|_, _| 0.0);
        assert!((fan[0] - 0.5).abs() < 1e-5, "r clamped to 0.5, got {}", fan[0]);
        assert!((fan[1] - 0.5).abs() < 1e-5, "g clamped to 0.5, got {}", fan[1]);
        assert!((fan[2] - 0.5).abs() < 1e-5, "b stays 0.5, got {}", fan[2]);
        assert!((fan[3] - 0.5).abs() < 1e-5, "a clamped to 0.5, got {}", fan[3]);
    }

    #[test]
    fn test_blend_node_modes() {
        // Content → Blend(B=red, weight=1) → Output: fully replaced by B.
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("red", 60.0, 90.0, NodeKind::MakeColor),
                ModifierNode::new("one", 60.0, 150.0, NodeKind::Constant {
                    mode: ConstantMode::Float,
                    value: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }),
                ModifierNode::new("mix", 140.0, 30.0, NodeKind::Blend { mode: GraphBlendMode::Linear, default_b: 0.0 }),
                ModifierNode::new("w", 60.0, 200.0, NodeKind::Constant { mode: ConstantMode::Float, value: 1.0, g: 0.0, b: 0.0, a: 1.0 }),
                ModifierNode::new("out", 240.0, 30.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "mix", "a"),
                NodeConnection::new("red", "value", "mix", "b"),
                NodeConnection::new("one", "value", "red", "r"),
                NodeConnection::new("w", "value", "mix", "weight"),
                NodeConnection::new("mix", "result", "out", "result"),
            ],
        };
        // Red's r is wired to 1.0; g/b/a pass the per-channel base. Linear at
        // w=1 returns B entirely.
        let out = graph.evaluate_color(0.1, 0.8, 0.4, 1.0, 0.0, &|_, _| 0.0);
        assert!((out[0] - 1.0).abs() < 1e-5, "r={}", out[0]);
        assert!((out[1] - 0.8).abs() < 1e-5, "g={}", out[1]);
        assert!((out[2] - 0.4).abs() < 1e-5, "b={}", out[2]);
        assert!((out[3] - 1.0).abs() < 1e-5, "a={}", out[3]);

        // Multiply at w=0.5: a + (2a - a) * 0.5 = 1.5 a.
        let w_graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("half", 60.0, 140.0, NodeKind::Constant { mode: ConstantMode::Float, value: 0.5, g: 0.0, b: 0.0, a: 1.0 }),
                ModifierNode::new("mix", 140.0, 30.0, NodeKind::Blend { mode: GraphBlendMode::Multiply, default_b: 2.0 }),
                ModifierNode::new("out", 240.0, 30.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "mix", "a"),
                NodeConnection::new("half", "value", "mix", "b"),
                NodeConnection::new("half", "value", "mix", "weight"),
                NodeConnection::new("mix", "result", "out", "result"),
            ],
        };
        let x = w_graph.evaluate_color(0.4, 0.4, 0.4, 0.4, 0.0, &|_, _| 0.0);
        // Multiply mixes toward the product a*b: 0.4 + (0.2 - 0.4) * 0.5 = 0.3.
        assert!((x[0] - 0.3).abs() < 1e-5, "0.4 + (0.2-0.4)*0.5 = 0.3, got {}", x[0]);
    }

    #[test]
    fn test_stockop_pointwise_node() {
        // Content → Posterize(steps=2) → Output, unwired `in` = base pixel.
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("po", 120.0, 0.0, NodeKind::StockOp { plugin: crate::StockPlugin::Posterize, params: vec![2.0] }),
                ModifierNode::new("out", 240.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "po", "in"),
                NodeConnection::new("po", "value", "out", "result"),
            ],
        };
        let out = graph.evaluate_color(0.7, 0.2, 0.9, 1.0, 0.0, &|_, _| 0.0);
        assert!(out[0] > 0.9, "r snaps 1, got {}", out[0]);
        assert!(out[1] < 0.1, "g snaps 0, got {}", out[1]);
        assert!(out[2] > 0.9, "b snaps 1, got {}", out[2]);
        assert!((out[3] - 1.0).abs() < 1e-5, "alpha untouched, got {}", out[3]);

        // Chained stock: Threshold then Posterize (nested color buses).
        let chain = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("th", 80.0, 0.0, NodeKind::StockOp { plugin: crate::StockPlugin::Threshold, params: vec![50.0, 0.0] }),
                ModifierNode::new("po", 160.0, 0.0, NodeKind::StockOp { plugin: crate::StockPlugin::Posterize, params: vec![2.0] }),
                ModifierNode::new("out", 240.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "th", "in"),
                NodeConnection::new("th", "value", "po", "in"),
                NodeConnection::new("po", "value", "out", "result"),
            ],
        };
        let white = chain.evaluate_color(0.9, 0.9, 0.9, 1.0, 0.0, &|_, _| 0.0);
        let black = chain.evaluate_color(0.1, 0.1, 0.1, 1.0, 0.0, &|_, _| 0.0);
        assert!(white[0] > 0.99, "bright → white, got {:?}", white);
        assert!(black[0] < 0.01, "dark → black, got {:?}", black);
    }

    #[test]
    fn test_graph_is_color_clean() {
        // Default passthrough stays clean (unreachable GetLayerFactor).
        assert!(ModifierGraph::default_passthrough().is_color_clean());

        let stock = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("po", 120.0, 0.0, NodeKind::StockOp { plugin: crate::StockPlugin::Posterize, params: vec![] }),
                ModifierNode::new("out", 240.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "po", "in"),
                NodeConnection::new("po", "value", "out", "result"),
            ],
        };
        assert!(stock.is_color_clean());

        // Math wired into a Blend weight makes the twin invalid.
        let mixed = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("math", 60.0, 90.0, NodeKind::Math { op: MathOp::Add, default_b: 0.0 }),
                ModifierNode::new("mix", 140.0, 30.0, NodeKind::Blend { mode: GraphBlendMode::Linear, default_b: 0.0 }),
                ModifierNode::new("out", 240.0, 30.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "mix", "a"),
                NodeConnection::new("math", "result", "mix", "weight"),
                NodeConnection::new("mix", "result", "out", "result"),
            ],
        };
        assert!(!mixed.is_color_clean());

        // Spatial stock (box blur) is never a GPU node.
        let blurred = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("bb", 120.0, 0.0, NodeKind::StockOp { plugin: crate::StockPlugin::BoxBlur, params: vec![] }),
                ModifierNode::new("out", 240.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "bb", "in"),
                NodeConnection::new("bb", "value", "out", "result"),
            ],
        };
        assert!(!blurred.is_color_clean());

        // Noise node reached through any fan-in disqualifies.
        let noisy = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("n", 60.0, 90.0, NodeKind::Noise { frequency: 1.0, amplitude: 1.0 }),
                ModifierNode::new("out", 240.0, 0.0, NodeKind::Output),
            ],
            connections: vec![NodeConnection::new("n", "result", "out", "result")],
        };
        assert!(!noisy.is_color_clean());
    }

    #[test]
    fn test_graph_nodes_roundtrip_serde() {
        use serde_json;
        let graph = ModifierGraph {
            nodes: vec![
                ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                ModifierNode::new("blend", 100.0, 0.0, NodeKind::Blend { mode: GraphBlendMode::Screen, default_b: 0.4 }),
                ModifierNode::new("po", 200.0, 0.0, NodeKind::StockOp { plugin: crate::StockPlugin::Posterize, params: vec![3.0, 5.0] }),
                ModifierNode::new("out", 300.0, 0.0, NodeKind::Output),
            ],
            connections: vec![
                NodeConnection::new("in", "value", "blend", "a"),
                NodeConnection::new("in", "value", "po", "in"),
                NodeConnection::new("po", "value", "blend", "b"),
                NodeConnection::new("blend", "result", "out", "result"),
            ],
        };
        let json = serde_json::to_string(&graph).unwrap();
        let back: ModifierGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(graph, back);
    }
}
