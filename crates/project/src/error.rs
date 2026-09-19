use std::fmt;

/// Errors that can occur during color operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorError {
    InvalidHexFormat(String),
    InvalidHexDigit(char),
}

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHexFormat(s) => write!(f, "invalid hex color format: '{s}'"),
            Self::InvalidHexDigit(c) => write!(f, "invalid hex digit: '{c}'"),
        }
    }
}

impl std::error::Error for ColorError {}

/// Errors that can occur during timecode operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeCodeError {
    InvalidFormat(String),
    InvalidComponent(String),
    InvalidFrameRate(String),
    DroppedFrame(String),
}

impl fmt::Display for TimeCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(s) => write!(f, "invalid timecode format: '{s}' (expected HH:MM:SS:FF or HH:MM:SS;FF)"),
            Self::InvalidComponent(s) => write!(f, "invalid timecode component: '{s}'"),
            Self::InvalidFrameRate(s) => write!(f, "invalid frame rate: '{s}' (must be > 0.0)"),
            Self::DroppedFrame(s) => write!(f, "dropped frame in timecode: '{s}'"),
        }
    }
}

impl std::error::Error for TimeCodeError {}

/// Validation errors for project and composition integrity.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    InvalidDimensions { width: u32, height: u32 },
    InvalidFrameRate(f64),
    InvalidDuration(String),
    InvalidLayerTiming {
        layer_id: String,
        in_point_frames: i64,
        out_point_frames: i64,
    },
    LayerNotFound(String),
    DuplicateLayerId(String),
    ParentNotFound {
        layer_id: String,
        parent_id: String,
    },
    SelfParenting(String),
    ParentCycleDetected {
        layer_id: String,
        cycle: Vec<String>,
    },
    DuplicateCompositionId(String),
    CompositionNotFound(String),
    DuplicateAssetId(String),
    AssetNotFound(String),
    NestedCompositionNotFound {
        layer_id: String,
        composition_id: String,
    },
    CircularNestedComposition {
        composition_id: String,
        cycle: Vec<String>,
    },
    IndexOutOfBounds {
        index: usize,
        len: usize,
    },
    InvalidWorkArea {
        in_point_frames: i64,
        out_point_frames: i64,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => {
                write!(f, "invalid dimensions: {width}x{height} (must be > 0)")
            }
            Self::InvalidFrameRate(fps) => {
                write!(f, "invalid frame rate: {fps} (must be > 0.0)")
            }
            Self::InvalidDuration(msg) => {
                write!(f, "invalid duration: {msg}")
            }
            Self::InvalidLayerTiming {
                layer_id,
                in_point_frames,
                out_point_frames,
            } => {
                write!(
                    f,
                    "invalid timing for layer '{layer_id}': in_point ({in_point_frames}) must be <= out_point ({out_point_frames})"
                )
            }
            Self::InvalidWorkArea {
                in_point_frames,
                out_point_frames,
            } => {
                write!(
                    f,
                    "invalid work area bounds: in_point ({in_point_frames}) must be <= out_point ({out_point_frames})"
                )
            }
            Self::LayerNotFound(id) => write!(f, "layer not found: '{id}'"),
            Self::DuplicateLayerId(id) => write!(f, "duplicate layer id: '{id}'"),
            Self::ParentNotFound { layer_id, parent_id } => {
                write!(f, "layer '{layer_id}' references non-existent parent '{parent_id}'")
            }
            Self::SelfParenting(id) => write!(f, "layer '{id}' cannot parent to itself"),
            Self::ParentCycleDetected { layer_id, cycle } => {
                write!(
                    f,
                    "parent cycle detected for layer '{layer_id}': {}",
                    cycle.join(" -> ")
                )
            }
            Self::DuplicateCompositionId(id) => write!(f, "duplicate composition id: '{id}'"),
            Self::CompositionNotFound(id) => write!(f, "composition not found: '{id}'"),
            Self::DuplicateAssetId(id) => write!(f, "duplicate asset id: '{id}'"),
            Self::AssetNotFound(id) => write!(f, "asset not found: '{id}'"),
            Self::NestedCompositionNotFound {
                layer_id,
                composition_id,
            } => {
                write!(
                    f,
                    "layer '{layer_id}' references non-existent nested composition '{composition_id}'"
                )
            }
            Self::CircularNestedComposition {
                composition_id,
                cycle,
            } => {
                write!(
                    f,
                    "circular nested composition reference for '{composition_id}': {}",
                    cycle.join(" -> ")
                )
            }
            Self::IndexOutOfBounds { index, len } => {
                write!(f, "index out of bounds: {index} (length: {len})")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Top-level project error type.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectError {
    Validation(ValidationError),
    Color(ColorError),
    TimeCode(TimeCodeError),
    Serialization(String),
    Io(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(e) => write!(f, "validation error: {e}"),
            Self::Color(e) => write!(f, "color error: {e}"),
            Self::TimeCode(e) => write!(f, "timecode error: {e}"),
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
            Self::Io(e) => write!(f, "i/o error: {e}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<ValidationError> for ProjectError {
    fn from(err: ValidationError) -> Self {
        Self::Validation(err)
    }
}

impl From<ColorError> for ProjectError {
    fn from(err: ColorError) -> Self {
        Self::Color(err)
    }
}

impl From<TimeCodeError> for ProjectError {
    fn from(err: TimeCodeError) -> Self {
        Self::TimeCode(err)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}
