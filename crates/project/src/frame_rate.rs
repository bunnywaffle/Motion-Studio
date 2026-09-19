use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Greatest common divisor helper.
const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Precise rational representation of video frame rate.
///
/// Frame rates in broadcast and film are rational numbers (e.g. 24000/1001 for 23.976 fps,
/// 30000/1001 for 29.97 fps). Representing them as rational numbers prevents accumulation of
/// floating point error during playback and editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRate {
    numerator: u32,
    denominator: u32,
    #[serde(default)]
    drop_frame: bool,
}

impl FrameRate {
    /// 23.976 fps standard cinema/film NTSC (24,000 / 1,001).
    pub const FPS_23_976: Self = Self {
        numerator: 24000,
        denominator: 1001,
        drop_frame: false,
    };

    /// 24.0 fps standard cinema/film (24 / 1).
    pub const FPS_24: Self = Self {
        numerator: 24,
        denominator: 1,
        drop_frame: false,
    };

    /// 25.0 fps standard PAL/SECAM television (25 / 1).
    pub const FPS_25: Self = Self {
        numerator: 25,
        denominator: 1,
        drop_frame: false,
    };

    /// 29.97 fps NTSC Non-Drop-Frame (30,000 / 1,001).
    pub const FPS_29_97_NDF: Self = Self {
        numerator: 30000,
        denominator: 1001,
        drop_frame: false,
    };

    /// 29.97 fps NTSC Drop-Frame (30,000 / 1,001 with SMPTE drop-frame compensation).
    pub const FPS_29_97_DF: Self = Self {
        numerator: 30000,
        denominator: 1001,
        drop_frame: true,
    };

    /// Default 29.97 fps (Non-Drop-Frame).
    pub const FPS_29_97: Self = Self::FPS_29_97_NDF;

    /// 30.0 fps standard computer video (30 / 1).
    pub const FPS_30: Self = Self {
        numerator: 30,
        denominator: 1,
        drop_frame: false,
    };

    /// 50.0 fps high-rate PAL (50 / 1).
    pub const FPS_50: Self = Self {
        numerator: 50,
        denominator: 1,
        drop_frame: false,
    };

    /// 59.94 fps high-rate NTSC Non-Drop-Frame (60,000 / 1,001).
    pub const FPS_59_94_NDF: Self = Self {
        numerator: 60000,
        denominator: 1001,
        drop_frame: false,
    };

    /// 59.94 fps high-rate NTSC Drop-Frame (60,000 / 1,001 with SMPTE drop-frame compensation).
    pub const FPS_59_94_DF: Self = Self {
        numerator: 60000,
        denominator: 1001,
        drop_frame: true,
    };

    /// Default 59.94 fps (Non-Drop-Frame).
    pub const FPS_59_94: Self = Self::FPS_59_94_NDF;

    /// 60.0 fps high-rate computer video (60 / 1).
    pub const FPS_60: Self = Self {
        numerator: 60,
        denominator: 1,
        drop_frame: false,
    };

    /// Construct a rational frame rate with numerator and denominator.
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        assert!(numerator > 0, "numerator must be > 0");
        assert!(denominator > 0, "denominator must be > 0");
        let g = gcd(numerator, denominator);
        Self {
            numerator: numerator / g,
            denominator: denominator / g,
            drop_frame: false,
        }
    }

    /// Construct a custom rational frame rate with optional drop frame flag.
    pub const fn custom(numerator: u32, denominator: u32) -> Self {
        Self::new(numerator, denominator)
    }

    /// Return a copy with drop-frame timing enabled or disabled.
    pub const fn with_drop_frame(mut self, drop_frame: bool) -> Self {
        self.drop_frame = drop_frame;
        self
    }

    /// Return the numerator of the frame rate.
    pub const fn numerator(&self) -> u32 {
        self.numerator
    }

    /// Return the denominator of the frame rate.
    pub const fn denominator(&self) -> u32 {
        self.denominator
    }

    /// Return whether this frame rate uses drop-frame timing.
    pub const fn is_drop_frame(&self) -> bool {
        self.drop_frame
    }

    /// Return the floating-point frames per second.
    pub fn as_f64(&self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    /// Return the nominal integer frame rate (e.g. 30 for 29.97, 60 for 59.94, 24 for 23.976).
    pub const fn nominal_fps(&self) -> u32 {
        (self.numerator + self.denominator / 2) / self.denominator
    }

    /// Return the number of frames dropped per minute (except 10th minute) in SMPTE drop frame.
    /// Returns 2 for 29.97 fps, 4 for 59.94 fps, and 0 for non-drop frame rates.
    pub const fn drop_frame_count(&self) -> u32 {
        if !self.drop_frame {
            0
        } else if self.nominal_fps() == 30 {
            2
        } else if self.nominal_fps() == 60 {
            4
        } else {
            0
        }
    }

    /// Infer standard FrameRate preset from a floating-point frame rate.
    pub fn from_fps(fps: f64) -> Self {
        if !fps.is_finite() || fps <= 0.0 {
            return Self::FPS_30;
        }

        // Match common NTSC fractional frame rates
        if (fps - 24000.0 / 1001.0).abs() < 1e-4 || (fps - 23.976).abs() < 1e-3 {
            Self::FPS_23_976
        } else if (fps - 30000.0 / 1001.0).abs() < 1e-4 || (fps - 29.97).abs() < 1e-3 {
            Self::FPS_29_97_NDF
        } else if (fps - 60000.0 / 1001.0).abs() < 1e-4 || (fps - 59.94).abs() < 1e-3 {
            Self::FPS_59_94_NDF
        } else if (fps - 24.0).abs() < 1e-4 {
            Self::FPS_24
        } else if (fps - 25.0).abs() < 1e-4 {
            Self::FPS_25
        } else if (fps - 30.0).abs() < 1e-4 {
            Self::FPS_30
        } else if (fps - 50.0).abs() < 1e-4 {
            Self::FPS_50
        } else if (fps - 60.0).abs() < 1e-4 {
            Self::FPS_60
        } else if (fps - fps.round()).abs() < 1e-4 {
            let rounded = fps.round() as u32;
            Self::new(rounded.max(1), 1)
        } else {
            let num = (fps * 1000.0).round() as u32;
            let den = 1000;
            Self::new(num, den)
        }
    }

    /// Return true if this matches one of the industry-standard frame rates.
    pub fn is_standard(&self) -> bool {
        let num = self.numerator;
        let den = self.denominator;
        (num == 24000 && den == 1001)
            || (num == 24 && den == 1)
            || (num == 25 && den == 1)
            || (num == 30000 && den == 1001)
            || (num == 30 && den == 1)
            || (num == 50 && den == 1)
            || (num == 60000 && den == 1001)
            || (num == 60 && den == 1)
    }

    /// Convert a frame index to exact seconds.
    pub fn frames_to_seconds(&self, frames: i64) -> f64 {
        (frames as f64 * self.denominator as f64) / self.numerator as f64
    }

    /// Convert continuous time in seconds to the nearest discrete frame index.
    pub fn seconds_to_frames(&self, seconds: f64) -> i64 {
        if !seconds.is_finite() {
            return 0;
        }
        ((seconds * self.numerator as f64) / self.denominator as f64).round() as i64
    }

    /// Return the single-frame duration as a `std::time::Duration`.
    pub fn frame_duration(&self) -> Duration {
        Duration::from_nanos(self.frame_duration_nanos() as u64)
    }

    /// Return the single-frame duration in nanoseconds.
    pub fn frame_duration_nanos(&self) -> u128 {
        (1_000_000_000u128 * self.denominator as u128) / self.numerator as u128
    }

    /// Convert nanoseconds from zero into an exact integer frame index and fractional subframe in `[0.0, 1.0)`.
    pub fn nanos_to_frames(&self, nanos: i128) -> (i64, f64) {
        let num = self.numerator as f64;
        let den = self.denominator as f64 * 1_000_000_000.0;

        let total_frames = (nanos as f64 * num) / den;
        let rounded = total_frames.round();
        if (total_frames - rounded).abs() < 1e-5 {
            (rounded as i64, 0.0)
        } else {
            let frame = total_frames.floor() as i64;
            let subframe = total_frames - frame as f64;
            (frame, subframe.clamp(0.0, 1.0))
        }
    }

    /// Convert a discrete frame index to exact nanoseconds.
    pub fn frame_to_nanos(&self, frame: i64) -> i128 {
        (frame as i128 * self.denominator as i128 * 1_000_000_000) / self.numerator as i128
    }
}

impl Default for FrameRate {
    fn default() -> Self {
        Self::FPS_30
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let df_suffix = if self.drop_frame { " DF" } else { "" };
        if self.denominator == 1 {
            write!(f, "{}{df_suffix} fps", self.numerator)
        } else if self.numerator == 24000 && self.denominator == 1001 {
            write!(f, "23.976{df_suffix} fps")
        } else if self.numerator == 30000 && self.denominator == 1001 {
            write!(f, "29.97{df_suffix} fps")
        } else if self.numerator == 60000 && self.denominator == 1001 {
            write!(f, "59.94{df_suffix} fps")
        } else {
            write!(
                f,
                "{:.3}{df_suffix} fps ({}/{})",
                self.as_f64(),
                self.numerator,
                self.denominator
            )
        }
    }
}
