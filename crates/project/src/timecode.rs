use crate::error::TimeCodeError;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Sub};

/// Standard representation of video timecode and frame timing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TimeCode {
    frames: i64,
    frame_rate: f64,
    #[serde(default)]
    drop_frame: bool,
}

impl TimeCode {
    /// Create a TimeCode from a frame count and frame rate.
    pub fn from_frames(frames: i64, frame_rate: f64) -> Self {
        let fps = if frame_rate > 0.0 && frame_rate.is_finite() {
            frame_rate
        } else {
            30.0
        };
        Self {
            frames,
            frame_rate: fps,
            drop_frame: false,
        }
    }

    /// Create a TimeCode at zero frames for a given frame rate.
    pub fn zero(frame_rate: f64) -> Self {
        Self::from_frames(0, frame_rate)
    }

    /// Create a TimeCode from seconds and frame rate.
    pub fn from_seconds(seconds: f64, frame_rate: f64) -> Self {
        let fps = if frame_rate > 0.0 && frame_rate.is_finite() {
            frame_rate
        } else {
            30.0
        };
        let frames = if seconds.is_finite() {
            (seconds * fps).round() as i64
        } else {
            0
        };
        Self {
            frames,
            frame_rate: fps,
            drop_frame: false,
        }
    }

    /// Return the raw frame count.
    pub const fn frames(&self) -> i64 {
        self.frames
    }

    /// Return the frame rate (frames per second).
    pub const fn frame_rate(&self) -> f64 {
        self.frame_rate
    }

    /// Return whether this timecode uses drop-frame notation.
    pub const fn is_drop_frame(&self) -> bool {
        self.drop_frame
    }

    /// Set whether this timecode uses drop-frame notation.
    pub fn with_drop_frame(mut self, drop_frame: bool) -> Self {
        self.drop_frame = drop_frame;
        self
    }

    /// Return the time in seconds.
    pub fn seconds(&self) -> f64 {
        if self.frame_rate > 0.0 {
            self.frames as f64 / self.frame_rate
        } else {
            0.0
        }
    }

    /// Parse a timecode string in `[ - ]HH:MM:SS:FF` or `[ - ]HH:MM:SS;FF` format.
    pub fn from_timecode_str(s: &str, frame_rate: f64) -> Result<Self, TimeCodeError> {
        if frame_rate <= 0.0 || !frame_rate.is_finite() {
            return Err(TimeCodeError::InvalidFrameRate(frame_rate.to_string()));
        }

        let trimmed = s.trim();
        let (is_negative, raw_s) = if let Some(stripped) = trimmed.strip_prefix('-') {
            (true, stripped.trim())
        } else {
            (false, trimmed)
        };

        let is_drop = raw_s.contains(';');
        let parts: Vec<&str> = raw_s.split([':', ';']).collect();
        if parts.len() != 4 {
            return Err(TimeCodeError::InvalidFormat(s.to_string()));
        }

        let parse_part = |idx: usize, name: &str| -> Result<i64, TimeCodeError> {
            let part_str = parts[idx].trim();
            let val = part_str
                .parse::<i64>()
                .map_err(|_| TimeCodeError::InvalidComponent(format!("{name}: '{part_str}'")))?;
            if val < 0 {
                return Err(TimeCodeError::InvalidComponent(format!(
                    "{name} must be non-negative: '{part_str}'"
                )));
            }
            Ok(val)
        };

        let hours = parse_part(0, "hours")?;
        let minutes = parse_part(1, "minutes")?;
        let seconds = parse_part(2, "seconds")?;
        let frames = parse_part(3, "frames")?;

        let fps_int = (frame_rate.round() as i64).max(1);
        let mut total_frames = ((hours * 60 + minutes) * 60 + seconds) * fps_int + frames;
        if is_negative {
            total_frames = -total_frames;
        }

        Ok(Self {
            frames: total_frames,
            frame_rate,
            drop_frame: is_drop,
        })
    }

    /// Format as standard `HH:MM:SS:FF` (or `HH:MM:SS;FF` for drop-frame).
    pub fn to_timecode_str(&self) -> String {
        let fps_int = (self.frame_rate.round() as i64).max(1);
        let is_negative = self.frames < 0;
        let total_frames = self.frames.unsigned_abs() as i64;

        let ff = total_frames % fps_int;
        let total_seconds = total_frames / fps_int;
        let ss = total_seconds % 60;
        let total_minutes = total_seconds / 60;
        let mm = total_minutes % 60;
        let hh = total_minutes / 60;

        let sep = if self.drop_frame { ';' } else { ':' };
        let sign = if is_negative { "-" } else { "" };
        format!("{sign}{hh:02}:{mm:02}:{ss:02}{sep}{ff:02}")
    }

    /// Resample this timecode to a new frame rate.
    pub fn resample(&self, new_frame_rate: f64) -> Self {
        Self::from_seconds(self.seconds(), new_frame_rate).with_drop_frame(self.drop_frame)
    }
}

impl Default for TimeCode {
    fn default() -> Self {
        Self::from_frames(0, 30.0)
    }
}

impl fmt::Display for TimeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_timecode_str())
    }
}

impl PartialEq for TimeCode {
    fn eq(&self, other: &Self) -> bool {
        if (self.frame_rate - other.frame_rate).abs() < 1e-5 {
            self.frames == other.frames
        } else {
            (self.seconds() - other.seconds()).abs() < 1e-7
        }
    }
}

impl PartialOrd for TimeCode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else {
            self.seconds().partial_cmp(&other.seconds())
        }
    }
}

impl Add for TimeCode {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        if (self.frame_rate - rhs.frame_rate).abs() < 1e-5 {
            Self {
                frames: self.frames + rhs.frames,
                frame_rate: self.frame_rate,
                drop_frame: self.drop_frame || rhs.drop_frame,
            }
        } else {
            // Normalize via seconds
            Self::from_seconds(self.seconds() + rhs.seconds(), self.frame_rate)
        }
    }
}

impl Sub for TimeCode {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        if (self.frame_rate - rhs.frame_rate).abs() < 1e-5 {
            Self {
                frames: self.frames - rhs.frames,
                frame_rate: self.frame_rate,
                drop_frame: self.drop_frame,
            }
        } else {
            Self::from_seconds(self.seconds() - rhs.seconds(), self.frame_rate)
        }
    }
}
