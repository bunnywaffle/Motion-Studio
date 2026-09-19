use crate::error::TimeCodeError;
use crate::frame_rate::FrameRate;
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
    /// Create a TimeCode from a frame count and floating-point frame rate.
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

    /// Create a TimeCode from a frame count and rational `FrameRate`.
    pub fn from_frame_rate(frames: i64, frame_rate: FrameRate) -> Self {
        Self {
            frames,
            frame_rate: frame_rate.as_f64(),
            drop_frame: frame_rate.is_drop_frame(),
        }
    }

    /// Create a TimeCode at zero frames for a given frame rate.
    pub fn zero(frame_rate: f64) -> Self {
        Self::from_frames(0, frame_rate)
    }

    /// Create a TimeCode at zero frames for a rational `FrameRate`.
    pub fn zero_with_rate(frame_rate: FrameRate) -> Self {
        Self::from_frame_rate(0, frame_rate)
    }

    /// Create a TimeCode from seconds and floating-point frame rate.
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

    /// Create a TimeCode from seconds and rational `FrameRate`.
    pub fn from_seconds_with_rate(seconds: f64, frame_rate: FrameRate) -> Self {
        let frames = frame_rate.seconds_to_frames(seconds);
        Self {
            frames,
            frame_rate: frame_rate.as_f64(),
            drop_frame: frame_rate.is_drop_frame(),
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

    /// Return the frame rate as a rational `FrameRate` structure.
    pub fn frame_rate_info(&self) -> FrameRate {
        FrameRate::from_fps(self.frame_rate).with_drop_frame(self.drop_frame)
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

    /// Parse a timecode string using a rational `FrameRate`.
    ///
    /// Respects the rational frame rate's drop-frame configuration even if the separator is `:`,
    /// and detects drop-frame timecodes if `;` is present. Validates component bounds.
    pub fn from_timecode_with_rate(s: &str, frame_rate: FrameRate) -> Result<Self, TimeCodeError> {
        if frame_rate.numerator() == 0 || frame_rate.denominator() == 0 {
            return Err(TimeCodeError::InvalidFrameRate(frame_rate.to_string()));
        }

        let trimmed = s.trim();
        let (is_negative, raw_s) = if let Some(stripped) = trimmed.strip_prefix('-') {
            (true, stripped.trim())
        } else {
            (false, trimmed)
        };

        let has_semicolon = raw_s.contains(';');
        let is_drop = has_semicolon || frame_rate.is_drop_frame();
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

        let rate_info = frame_rate.with_drop_frame(is_drop);
        let mut total_frames = if is_drop && rate_info.drop_frame_count() > 0 {
            drop_frame_smpte_to_frame(
                hours,
                minutes,
                seconds,
                frames,
                rate_info.nominal_fps(),
                rate_info.drop_frame_count(),
            )?
        } else {
            let fps_int = rate_info.nominal_fps() as i64;
            if frames >= fps_int {
                return Err(TimeCodeError::InvalidComponent(format!(
                    "frames must be < {fps_int}: '{frames}'"
                )));
            }
            if seconds >= 60 {
                return Err(TimeCodeError::InvalidComponent(format!(
                    "seconds must be < 60: '{seconds}'"
                )));
            }
            if minutes >= 60 {
                return Err(TimeCodeError::InvalidComponent(format!(
                    "minutes must be < 60: '{minutes}'"
                )));
            }
            ((hours * 60 + minutes) * 60 + seconds) * fps_int + frames
        };

        if is_negative {
            total_frames = -total_frames;
        }

        Ok(Self {
            frames: total_frames,
            frame_rate: rate_info.as_f64(),
            drop_frame: is_drop,
        })
    }

    /// Parse a timecode string in `[ - ]HH:MM:SS:FF` or `[ - ]HH:MM:SS;FF` format.
    ///
    /// If the string contains a semicolon `;` separator, it is parsed according to
    /// SMPTE drop-frame rules when the frame rate is standard drop-frame (29.97 or 59.94).
    pub fn from_timecode_str(s: &str, frame_rate: f64) -> Result<Self, TimeCodeError> {
        if frame_rate <= 0.0 || !frame_rate.is_finite() {
            return Err(TimeCodeError::InvalidFrameRate(frame_rate.to_string()));
        }
        let fr = FrameRate::from_fps(frame_rate).with_drop_frame(s.contains(';'));
        Self::from_timecode_with_rate(s, fr)
    }

    /// Parse a timecode string using a rational `FrameRate`.
    pub fn from_smpte(s: &str, frame_rate: FrameRate) -> Result<Self, TimeCodeError> {
        Self::from_timecode_with_rate(s, frame_rate)
    }

    /// Format as standard `HH:MM:SS:FF` (or `HH:MM:SS;FF` for drop-frame).
    ///
    /// For drop-frame frame rates (29.97 DF or 59.94 DF), formats according to SMPTE 12M drop-frame
    /// specification where dropped frame numbers are omitted at minute marks (except 10th minutes).
    pub fn to_timecode_str(&self) -> String {
        let is_negative = self.frames < 0;
        let total_frames = self.frames.unsigned_abs() as i64;
        let sign = if is_negative { "-" } else { "" };

        let rate_info = self.frame_rate_info();
        if self.drop_frame && rate_info.drop_frame_count() > 0 {
            let (hh, mm, ss, ff) = frame_to_drop_frame_smpte(
                total_frames,
                rate_info.nominal_fps(),
                rate_info.drop_frame_count(),
            );
            format!("{sign}{hh:02}:{mm:02}:{ss:02};{ff:02}")
        } else {
            let fps_int = (self.frame_rate.round() as i64).max(1);
            let ff = total_frames % fps_int;
            let total_seconds = total_frames / fps_int;
            let ss = total_seconds % 60;
            let total_minutes = total_seconds / 60;
            let mm = total_minutes % 60;
            let hh = total_minutes / 60;

            let sep = if self.drop_frame { ';' } else { ':' };
            format!("{sign}{hh:02}:{mm:02}:{ss:02}{sep}{ff:02}")
        }
    }

    /// Alias for `to_timecode_str`.
    pub fn to_smpte_str(&self) -> String {
        self.to_timecode_str()
    }

    /// Resample this timecode to a new frame rate.
    pub fn resample(&self, new_frame_rate: f64) -> Self {
        Self::from_seconds(self.seconds(), new_frame_rate).with_drop_frame(self.drop_frame)
    }

    /// Resample this timecode to a new rational `FrameRate`.
    pub fn resample_with_rate(&self, new_frame_rate: FrameRate) -> Self {
        Self::from_seconds_with_rate(self.seconds(), new_frame_rate)
            .with_drop_frame(new_frame_rate.is_drop_frame())
    }
}

/// Convert a non-negative frame index to SMPTE drop-frame components (HH, MM, SS, FF).
pub fn frame_to_drop_frame_smpte(
    frames: i64,
    nominal_fps: u32,
    drop_count: u32,
) -> (u32, u32, u32, u32) {
    let frames_per_min = (nominal_fps * 60 - drop_count) as i64;
    let frames_per_10min = (nominal_fps * 60 * 10 - drop_count * 9) as i64;

    let d = frames / frames_per_10min;
    let m = frames % frames_per_10min;

    let f = if m > drop_count as i64 {
        frames + (drop_count as i64 * 9 * d)
            + drop_count as i64 * ((m - drop_count as i64) / frames_per_min)
    } else {
        frames + (drop_count as i64 * 9 * d)
    };

    let ff = (f % nominal_fps as i64) as u32;
    let total_seconds = f / nominal_fps as i64;
    let ss = (total_seconds % 60) as u32;
    let total_minutes = total_seconds / 60;
    let mm = (total_minutes % 60) as u32;
    let hh = (total_minutes / 60) as u32;

    (hh, mm, ss, ff)
}

/// Convert SMPTE drop-frame components (HH, MM, SS, FF) back to an exact frame index.
/// Returns an error if the timecode represents a dropped frame number (e.g. frame 0 or 1 in minute 1 of 29.97 DF).
pub fn drop_frame_smpte_to_frame(
    hh: i64,
    mm: i64,
    ss: i64,
    ff: i64,
    nominal_fps: u32,
    drop_count: u32,
) -> Result<i64, TimeCodeError> {
    if mm >= 60 {
        return Err(TimeCodeError::InvalidComponent(format!(
            "minutes must be < 60: '{mm}'"
        )));
    }
    if ss >= 60 {
        return Err(TimeCodeError::InvalidComponent(format!(
            "seconds must be < 60: '{ss}'"
        )));
    }
    if ff >= nominal_fps as i64 {
        return Err(TimeCodeError::InvalidComponent(format!(
            "frames must be < {nominal_fps}: '{ff}'"
        )));
    }
    if ss == 0 && ff < drop_count as i64 && (mm % 10 != 0) {
        return Err(TimeCodeError::DroppedFrame(format!(
            "frame {ff} was dropped at {hh:02}:{mm:02}:{ss:02}"
        )));
    }

    let total_minutes = hh * 60 + mm;
    let drop_minutes = total_minutes - (total_minutes / 10);
    let nominal_frames = ((hh * 60 + mm) * 60 + ss) * nominal_fps as i64 + ff;
    let frames = nominal_frames - (drop_minutes * drop_count as i64);

    Ok(frames)
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
                .with_drop_frame(self.drop_frame || rhs.drop_frame)
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
                .with_drop_frame(self.drop_frame)
        }
    }
}

