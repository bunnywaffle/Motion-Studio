use crate::color::Color;
use crate::timecode::TimeCode;
use serde::{Deserialize, Serialize};

/// A marker placed along the timeline on a composition or layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub time: TimeCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<TimeCode>,
    pub comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

impl Marker {
    /// Create a new point marker (zero duration).
    pub fn new(id: impl Into<String>, time: TimeCode, comment: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            time,
            duration: None,
            comment: comment.into(),
            color: None,
        }
    }

    /// Set an optional spanned duration for this marker.
    pub fn with_duration(mut self, duration: TimeCode) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set an optional display color for this marker.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}
