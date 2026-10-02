use crate::color::Color;
use crate::timecode::TimeCode;
use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

/// A trait for types that can be interpolated over time.
pub trait Interpolate: Clone {
    /// Linearly interpolate between `self` and `other` with factor `t` in `[0.0, 1.0]`.
    fn lerp(&self, other: &Self, t: f32) -> Self;

    /// Step / hold interpolation.
    /// In standard motion graphics, hold returns `self` for `t < 1.0`, and `other` at `t >= 1.0`.
    fn step(&self, other: &Self, t: f32) -> Self {
        if t >= 1.0 {
            other.clone()
        } else {
            self.clone()
        }
    }
}

impl Interpolate for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Interpolate for f64 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * (t as f64)
    }
}

impl Interpolate for Vec2 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Vec2::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

impl Interpolate for Color {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Color::rgba(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }
}

impl Interpolate for bool {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        if t >= 0.5 {
            *other
        } else {
            *self
        }
    }
}

impl Interpolate for String {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        if t >= 1.0 {
            other.clone()
        } else {
            self.clone()
        }
    }
}

/// The interpolation algorithm applied to the segment following this keyframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyframeInterpolation {
    /// Holds the keyframe's value constant until the next keyframe is reached.
    Hold,
    /// Linear progression between this keyframe and the next.
    #[default]
    Linear,
    /// Cubic Bezier curve progression governed by in/out tangent control handles.
    Bezier,
}

/// A 2D normalized tangent control handle for shaping Bezier interpolation curves.
///
/// `x` represents normalized time influence (typically `[0.0, 1.0]`).
/// `y` represents normalized value influence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeyframeTangent {
    pub x: f32,
    pub y: f32,
}

impl KeyframeTangent {
    /// Create a tangent handle with explicit x and y coordinates.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Standard linear outgoing tangent (1/3, 1/3).
    pub const fn linear_out() -> Self {
        Self {
            x: 1.0 / 3.0,
            y: 1.0 / 3.0,
        }
    }

    /// Standard linear incoming tangent (2/3, 2/3).
    pub const fn linear_in() -> Self {
        Self {
            x: 2.0 / 3.0,
            y: 2.0 / 3.0,
        }
    }

    /// Standard ease-in-out outgoing handle (0.42, 0.0).
    pub const fn ease_in_out_out() -> Self {
        Self { x: 0.42, y: 0.0 }
    }

    /// Standard ease-in-out incoming handle (0.58, 1.0).
    pub const fn ease_in_out_in() -> Self {
        Self { x: 0.58, y: 1.0 }
    }

    /// Standard ease-in outgoing handle (0.42, 0.0).
    pub const fn ease_in_out() -> Self {
        Self { x: 0.42, y: 0.0 }
    }

    /// Standard ease-in incoming handle (1.0, 1.0).
    pub const fn ease_in_in() -> Self {
        Self { x: 1.0, y: 1.0 }
    }

    /// Standard ease-out outgoing handle (0.0, 0.0).
    pub const fn ease_out_out() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    /// Standard ease-out incoming handle (0.58, 1.0).
    pub const fn ease_out_in() -> Self {
        Self { x: 0.58, y: 1.0 }
    }
}

impl Default for KeyframeTangent {
    fn default() -> Self {
        Self::linear_out()
    }
}

/// Extrapolation behavior when evaluating a property outside its keyframe time bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Extrapolation {
    /// Hold the boundary keyframe value (standard default in motion graphics).
    #[default]
    Hold,
    /// Linearly project using the slope of the boundary keyframe segment.
    Linear,
    /// Periodically loop / cycle the animation over the keyframe duration.
    Cycle,
    /// Oscillate back and forth between the start and end keyframes.
    PingPong,
}

/// A keyframe holding a value at a specific point in time, with easing and control handles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe<T> {
    pub time: TimeCode,
    #[serde(default)]
    pub subframe: f32,
    pub value: T,
    #[serde(default)]
    pub interpolation: KeyframeInterpolation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_tangent: Option<KeyframeTangent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_tangent: Option<KeyframeTangent>,
}

impl<T> Keyframe<T> {
    /// Create a new keyframe with linear interpolation.
    pub fn new(time: TimeCode, value: T) -> Self {
        Self {
            time,
            subframe: 0.0,
            value,
            interpolation: KeyframeInterpolation::Linear,
            in_tangent: None,
            out_tangent: None,
        }
    }

    /// Create a new keyframe with explicit interpolation type.
    pub fn with_interpolation(
        time: TimeCode,
        value: T,
        interpolation: KeyframeInterpolation,
    ) -> Self {
        Self {
            time,
            subframe: 0.0,
            value,
            interpolation,
            in_tangent: None,
            out_tangent: None,
        }
    }

    /// Create a hold / step keyframe.
    pub fn hold(time: TimeCode, value: T) -> Self {
        Self::with_interpolation(time, value, KeyframeInterpolation::Hold)
    }

    /// Create a linear keyframe.
    pub fn linear(time: TimeCode, value: T) -> Self {
        Self::with_interpolation(time, value, KeyframeInterpolation::Linear)
    }

    /// Create a Bezier keyframe with incoming and outgoing control handles.
    pub fn bezier(
        time: TimeCode,
        value: T,
        in_tangent: Option<KeyframeTangent>,
        out_tangent: Option<KeyframeTangent>,
    ) -> Self {
        Self {
            time,
            subframe: 0.0,
            value,
            interpolation: KeyframeInterpolation::Bezier,
            in_tangent,
            out_tangent,
        }
    }

    /// Create a keyframe from floating-point seconds and a frame rate.
    pub fn from_seconds(seconds: f64, frame_rate: f64, value: T) -> Self {
        let fps = if frame_rate > 0.0 && frame_rate.is_finite() {
            frame_rate
        } else {
            30.0
        };
        let total_frames = seconds * fps;
        let base_frames = total_frames.floor() as i64;
        let subframe = (total_frames - base_frames as f64) as f32;
        Self {
            time: TimeCode::from_frames(base_frames, fps),
            subframe,
            value,
            interpolation: KeyframeInterpolation::Linear,
            in_tangent: None,
            out_tangent: None,
        }
    }

    /// Builder to set subframe offset.
    pub fn with_subframe(mut self, subframe: f32) -> Self {
        self.subframe = subframe;
        self
    }

    /// Builder to set outgoing tangent.
    pub fn with_out_tangent(mut self, out_tangent: KeyframeTangent) -> Self {
        self.out_tangent = Some(out_tangent);
        self
    }

    /// Builder to set incoming tangent.
    pub fn with_in_tangent(mut self, in_tangent: KeyframeTangent) -> Self {
        self.in_tangent = Some(in_tangent);
        self
    }

    /// Return time in floating-point seconds, incorporating subframe offset.
    pub fn time_seconds(&self) -> f64 {
        self.time.seconds() + (self.subframe as f64 / self.time.frame_rate())
    }

    /// Return total time in frames as a floating-point number.
    pub fn time_frames_f64(&self) -> f64 {
        self.time.frames() as f64 + self.subframe as f64
    }
}

/// Evaluates a 1D cubic Bezier easing curve defined by normalized control points `p1` and `p2`.
///
/// Input `t` is the normalized time factor in `[0.0, 1.0]`.
/// Returns the eased factor `s` (typically `[0.0, 1.0]`, but may overshoot if handles exceed boundaries).
pub fn evaluate_cubic_bezier(t: f32, p1: KeyframeTangent, p2: KeyframeTangent) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }

    let x1 = p1.x.clamp(0.0, 1.0);
    let y1 = p1.y;
    let x2 = p2.x.clamp(0.0, 1.0);
    let y2 = p2.y;

    // Linear shortcut: if handles lie on the diagonal
    if (x1 - y1).abs() < 1e-5 && (x2 - y2).abs() < 1e-5 {
        return t;
    }

    // Cubic Bezier polynomial coefficients for x:
    // x(theta) = 3*(1-theta)^2 * theta * x1 + 3*(1-theta) * theta^2 * x2 + theta^3
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    let sample_curve_x = |theta: f32| -> f32 { ((ax * theta + bx) * theta + cx) * theta };
    let sample_curve_derivative_x =
        |theta: f32| -> f32 { (3.0 * ax * theta + 2.0 * bx) * theta + cx };

    // Solve for theta such that sample_curve_x(theta) == t
    // Step 1: Newton-Raphson iteration
    let mut theta = t;
    for _ in 0..8 {
        let x_err = sample_curve_x(theta) - t;
        if x_err.abs() < 1e-6 {
            break;
        }
        let d_x = sample_curve_derivative_x(theta);
        if d_x.abs() < 1e-6 {
            break; // Slope too flat, fall back to bisection
        }
        theta = (theta - x_err / d_x).clamp(0.0, 1.0);
    }

    // Step 2: Bisection fallback if Newton did not fully converge
    if (sample_curve_x(theta) - t).abs() > 1e-6 {
        let mut t_low = 0.0f32;
        let mut t_high = 1.0f32;
        theta = t;
        for _ in 0..24 {
            let x_val = sample_curve_x(theta);
            if (x_val - t).abs() < 1e-7 {
                break;
            }
            if x_val > t {
                t_high = theta;
            } else {
                t_low = theta;
            }
            theta = (t_low + t_high) * 0.5;
        }
    }

    // Evaluate y(theta):
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    ((ay * theta + by) * theta + cy) * theta
}

/// Interpolate between two keyframes at an absolute time `t` (in seconds).
pub fn interpolate_keyframes<T: Interpolate>(k1: &Keyframe<T>, k2: &Keyframe<T>, t: f64) -> T {
    let t1 = k1.time_seconds();
    let t2 = k2.time_seconds();

    if t2 <= t1 {
        return k2.value.clone();
    }

    let factor = (((t - t1) / (t2 - t1)) as f32).clamp(0.0, 1.0);

    match k1.interpolation {
        KeyframeInterpolation::Hold => k1.value.step(&k2.value, factor),
        KeyframeInterpolation::Linear => k1.value.lerp(&k2.value, factor),
        KeyframeInterpolation::Bezier => {
            let out_tan = k1.out_tangent.unwrap_or_else(KeyframeTangent::linear_out);
            let in_tan = k2.in_tangent.unwrap_or_else(KeyframeTangent::linear_in);
            let eased_t = evaluate_cubic_bezier(factor, out_tan, in_tan);
            k1.value.lerp(&k2.value, eased_t)
        }
    }
}

/// Evaluate an entire keyframe track at arbitrary floating-point seconds with extrapolation.
pub fn evaluate_keyframe_track<T: Interpolate>(
    keyframes: &[Keyframe<T>],
    time_seconds: f64,
    static_value: &T,
    pre_extrap: Extrapolation,
    post_extrap: Extrapolation,
) -> T {
    if keyframes.is_empty() {
        return static_value.clone();
    }

    if keyframes.len() == 1 {
        return keyframes[0].value.clone();
    }

    let t_first = keyframes[0].time_seconds();
    let t_last = keyframes.last().unwrap().time_seconds();
    let duration = t_last - t_first;

    // 1. Before first keyframe
    if time_seconds < t_first {
        match pre_extrap {
            Extrapolation::Hold => return keyframes[0].value.clone(),
            Extrapolation::Linear => {
                let k0 = &keyframes[0];
                let k1 = &keyframes[1];
                let dt = k1.time_seconds() - k0.time_seconds();
                if dt > 1e-6 {
                    let factor = ((time_seconds - k0.time_seconds()) / dt) as f32;
                    return k0.value.lerp(&k1.value, factor);
                } else {
                    return k0.value.clone();
                }
            }
            Extrapolation::Cycle => {
                if duration > 1e-6 {
                    let offset = time_seconds - t_first;
                    let wrapped = t_first + ((offset % duration) + duration) % duration;
                    return evaluate_keyframe_track(
                        keyframes,
                        wrapped,
                        static_value,
                        Extrapolation::Hold,
                        Extrapolation::Hold,
                    );
                } else {
                    return keyframes[0].value.clone();
                }
            }
            Extrapolation::PingPong => {
                if duration > 1e-6 {
                    let offset = (time_seconds - t_first).abs();
                    let cycle_count = (offset / duration).floor() as i64;
                    let remainder = offset % duration;
                    let wrapped = if cycle_count % 2 == 0 {
                        t_first + remainder
                    } else {
                        t_last - remainder
                    };
                    return evaluate_keyframe_track(
                        keyframes,
                        wrapped,
                        static_value,
                        Extrapolation::Hold,
                        Extrapolation::Hold,
                    );
                } else {
                    return keyframes[0].value.clone();
                }
            }
        }
    }

    // 2. After last keyframe
    if time_seconds >= t_last {
        match post_extrap {
            Extrapolation::Hold => return keyframes.last().unwrap().value.clone(),
            Extrapolation::Linear => {
                let n = keyframes.len();
                let k_prev = &keyframes[n - 2];
                let k_last = &keyframes[n - 1];
                let dt = k_last.time_seconds() - k_prev.time_seconds();
                if dt > 1e-6 {
                    let factor = ((time_seconds - k_prev.time_seconds()) / dt) as f32;
                    return k_prev.value.lerp(&k_last.value, factor);
                } else {
                    return k_last.value.clone();
                }
            }
            Extrapolation::Cycle => {
                if duration > 1e-6 {
                    let offset = time_seconds - t_first;
                    let wrapped = t_first + (offset % duration);
                    return evaluate_keyframe_track(
                        keyframes,
                        wrapped,
                        static_value,
                        Extrapolation::Hold,
                        Extrapolation::Hold,
                    );
                } else {
                    return keyframes.last().unwrap().value.clone();
                }
            }
            Extrapolation::PingPong => {
                if duration > 1e-6 {
                    let offset = time_seconds - t_first;
                    let cycle_count = (offset / duration).floor() as i64;
                    let remainder = offset % duration;
                    let wrapped = if cycle_count % 2 == 0 {
                        t_first + remainder
                    } else {
                        t_last - remainder
                    };
                    return evaluate_keyframe_track(
                        keyframes,
                        wrapped,
                        static_value,
                        Extrapolation::Hold,
                        Extrapolation::Hold,
                    );
                } else {
                    return keyframes.last().unwrap().value.clone();
                }
            }
        }
    }

    // 3. Within keyframe bounds: find interval [i, i+1]
    let idx = keyframes
        .iter()
        .rposition(|k| k.time_seconds() <= time_seconds)
        .unwrap_or(0);

    if idx >= keyframes.len() - 1 {
        return keyframes.last().unwrap().value.clone();
    }

    let k1 = &keyframes[idx];
    let k2 = &keyframes[idx + 1];

    if (k1.time_seconds() - time_seconds).abs() < 1e-6 {
        return k1.value.clone();
    }

    interpolate_keyframes(k1, k2, time_seconds)
}
