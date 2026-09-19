use crate::composition::Composition;
use crate::error::ValidationError;
use crate::frame_rate::FrameRate;
use crate::marker::Marker;
use crate::timecode::TimeCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Playback transport states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlaybackState {
    /// Playback is paused at current position.
    #[default]
    Paused,
    /// Playback is actively running forward or backward.
    Playing,
    /// User is actively dragging or scrubbing the playhead.
    Scrubbing,
}

impl PlaybackState {
    pub const fn is_playing(&self) -> bool {
        matches!(self, Self::Playing)
    }

    pub const fn is_paused(&self) -> bool {
        matches!(self, Self::Paused)
    }

    pub const fn is_scrubbing(&self) -> bool {
        matches!(self, Self::Scrubbing)
    }
}

/// Loop behaviors when reaching the boundary of the work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LoopMode {
    /// Wrap around from work_area_out to work_area_in (or vice versa in reverse).
    #[default]
    Loop,
    /// Play once to the boundary and automatically pause.
    Once,
    /// Bounce back and forth between work area boundaries by reversing playback speed.
    PingPong,
}

/// Direction of transport movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlaybackDirection {
    #[default]
    Forward,
    Reverse,
}

impl PlaybackDirection {
    pub const fn is_forward(&self) -> bool {
        matches!(self, Self::Forward)
    }

    pub const fn is_reverse(&self) -> bool {
        matches!(self, Self::Reverse)
    }

    pub fn toggle(&mut self) {
        *self = match self {
            Self::Forward => Self::Reverse,
            Self::Reverse => Self::Forward,
        };
    }
}

/// Work area definition specifying in and out bounds for playback and rendering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkArea {
    pub in_point: TimeCode,
    pub out_point: TimeCode,
}

impl WorkArea {
    /// Create a new work area from in and out points.
    /// Returns an error if `in_point > out_point`.
    pub fn new(in_point: TimeCode, out_point: TimeCode) -> Result<Self, ValidationError> {
        if in_point.frames() > out_point.frames() {
            return Err(ValidationError::InvalidWorkArea {
                in_point_frames: in_point.frames(),
                out_point_frames: out_point.frames(),
            });
        }
        Ok(Self { in_point, out_point })
    }

    /// Create a work area span, automatically swapping if in_point > out_point.
    pub fn from_span(a: TimeCode, b: TimeCode) -> Self {
        if a.frames() <= b.frames() {
            Self { in_point: a, out_point: b }
        } else {
            Self { in_point: b, out_point: a }
        }
    }

    /// Duration of the work area as a TimeCode.
    pub fn duration(&self) -> TimeCode {
        self.out_point - self.in_point
    }

    /// Duration of the work area in frames.
    pub fn duration_frames(&self) -> i64 {
        self.out_point.frames() - self.in_point.frames()
    }

    /// Duration of the work area in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.out_point.seconds() - self.in_point.seconds()
    }

    /// Return true if the given timecode falls within the work area `[in_point, out_point]`.
    pub fn contains(&self, time: TimeCode) -> bool {
        time.frames() >= self.in_point.frames() && time.frames() <= self.out_point.frames()
    }

    /// Clamp a timecode to lie within the work area.
    pub fn clamp(&self, time: TimeCode) -> TimeCode {
        if time.frames() < self.in_point.frames() {
            self.in_point
        } else if time.frames() > self.out_point.frames() {
            self.out_point
        } else {
            time
        }
    }
}

/// Result returned from a single `PlaybackClock::tick()` call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockTickResult {
    /// The quantized timecode after the tick.
    pub timecode: TimeCode,
    /// The discrete frame index after the tick.
    pub frame: i64,
    /// Whether the quantized frame index changed during this tick.
    pub frame_changed: bool,
    /// Whether a loop wrap-around or PingPong bounce occurred during this tick.
    pub looped: bool,
    /// Whether playback reached the end and stopped (e.g. In `Once` mode).
    pub reached_end: bool,
}

/// High-precision playback clock and timeline transport.
///
/// Tracks continuous wall-clock time with integer nanosecond precision to prevent
/// any floating-point drift over arbitrary playback durations. Provides frame quantization,
/// subframe evaluation for motion blur/physics, work-area looping/ping-ponging,
/// frame stepping, and marker/keyframe jumping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackClock {
    /// Current transport state.
    state: PlaybackState,
    /// Playback speed multiplier (1.0 = 100%, 0.5 = 50%, 2.0 = 200%, negative = reverse).
    speed: f64,
    /// Active rational frame rate.
    frame_rate: FrameRate,
    /// Exact high-precision position in integer nanoseconds from time zero.
    position_nanos: i128,
    /// Current quantized frame index.
    current_frame: i64,
    /// Fractional subframe offset in `[0.0, 1.0)` of the current frame.
    subframe: f64,
    /// Composition timeline start timecode.
    comp_start: TimeCode,
    /// Composition duration timecode.
    comp_duration: TimeCode,
    /// Active work area bounds for looping.
    work_area: WorkArea,
    /// Active loop mode.
    loop_mode: LoopMode,
    /// Direction of playback.
    direction: PlaybackDirection,
}

impl PlaybackClock {
    /// Create a new playback clock with a given rational frame rate and composition duration.
    pub fn new(frame_rate: FrameRate, comp_duration: TimeCode) -> Self {
        let comp_start = TimeCode::from_frame_rate(0, frame_rate);
        let work_area = WorkArea {
            in_point: comp_start,
            out_point: comp_duration,
        };
        Self {
            state: PlaybackState::Paused,
            speed: 1.0,
            frame_rate,
            position_nanos: 0,
            current_frame: 0,
            subframe: 0.0,
            comp_start,
            comp_duration,
            work_area,
            loop_mode: LoopMode::Loop,
            direction: PlaybackDirection::Forward,
        }
    }

    /// Create a playback clock initialized from a Composition.
    pub fn from_composition(comp: &Composition) -> Self {
        let frame_rate = FrameRate::from_fps(comp.frame_rate);
        let comp_start = TimeCode::from_frame_rate(0, frame_rate);
        let comp_duration = comp.duration;
        let work_area = WorkArea {
            in_point: comp_start,
            out_point: comp_duration,
        };
        Self {
            state: PlaybackState::Paused,
            speed: 1.0,
            frame_rate,
            position_nanos: 0,
            current_frame: 0,
            subframe: 0.0,
            comp_start,
            comp_duration,
            work_area,
            loop_mode: LoopMode::Loop,
            direction: PlaybackDirection::Forward,
        }
    }

    // --- State and Playback Queries ---

    /// Return the current playback state.
    pub const fn state(&self) -> PlaybackState {
        self.state
    }

    /// Return true if the transport is playing.
    pub const fn is_playing(&self) -> bool {
        self.state.is_playing()
    }

    /// Return true if the transport is paused.
    pub const fn is_paused(&self) -> bool {
        self.state.is_paused()
    }

    /// Return true if the user is scrubbing.
    pub const fn is_scrubbing(&self) -> bool {
        self.state.is_scrubbing()
    }

    /// Return the current playback speed multiplier.
    pub const fn speed(&self) -> f64 {
        self.speed
    }

    /// Return the current playback direction.
    pub const fn direction(&self) -> PlaybackDirection {
        self.direction
    }

    /// Return the active rational frame rate.
    pub const fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    /// Return the discrete quantized frame index.
    pub const fn current_frame(&self) -> i64 {
        self.current_frame
    }

    /// Return the fractional subframe offset in `[0.0, 1.0)`.
    pub const fn subframe(&self) -> f64 {
        self.subframe
    }

    /// Return continuous playback position in seconds.
    pub fn position_seconds(&self) -> f64 {
        self.position_nanos as f64 / 1_000_000_000.0
    }

    /// Return continuous position in integer nanoseconds.
    pub const fn position_nanos(&self) -> i128 {
        self.position_nanos
    }

    /// Return the current quantized `TimeCode`.
    pub fn timecode(&self) -> TimeCode {
        TimeCode::from_frames(self.current_frame, self.frame_rate.as_f64())
            .with_drop_frame(self.frame_rate.is_drop_frame())
    }

    /// Return the active work area bounds.
    pub const fn work_area(&self) -> WorkArea {
        self.work_area
    }

    /// Return the work area in point.
    pub const fn work_area_in(&self) -> TimeCode {
        self.work_area.in_point
    }

    /// Return the work area out point.
    pub const fn work_area_out(&self) -> TimeCode {
        self.work_area.out_point
    }

    /// Return the active loop mode.
    pub const fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// Return composition start timecode.
    pub const fn comp_start(&self) -> TimeCode {
        self.comp_start
    }

    /// Return composition duration timecode.
    pub const fn comp_duration(&self) -> TimeCode {
        self.comp_duration
    }

    // --- Transport Controls ---

    /// Start playback forward (or in the direction of `speed`).
    pub fn play(&mut self) {
        if self.state != PlaybackState::Playing {
            // Wrap to beginning if play is triggered while at or past the end
            if self.speed > 0.0 && self.current_frame >= self.work_area.out_point.frames() {
                self.jump_to_start();
            } else if self.speed < 0.0 && self.current_frame <= self.work_area.in_point.frames() {
                self.jump_to_end();
            }
            self.state = PlaybackState::Playing;
        }
    }

    /// Start reverse playback with negative speed.
    pub fn play_reverse(&mut self) {
        if self.speed > 0.0 {
            self.speed = -self.speed;
        } else if self.speed == 0.0 {
            self.speed = -1.0;
        }
        self.direction = PlaybackDirection::Reverse;
        if self.current_frame <= self.work_area.in_point.frames() {
            self.jump_to_end();
        }
        self.state = PlaybackState::Playing;
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.state = PlaybackState::Paused;
    }

    /// Toggle between Playing and Paused.
    pub fn toggle_playback(&mut self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Set playback speed multiplier. Automatically updates direction.
    pub fn set_speed(&mut self, speed: f64) {
        if speed.is_finite() {
            self.speed = speed;
            self.direction = if speed >= 0.0 {
                PlaybackDirection::Forward
            } else {
                PlaybackDirection::Reverse
            };
        }
    }

    /// Reverse playback speed direction.
    pub fn reverse(&mut self) {
        self.set_speed(-self.speed);
    }

    /// Set loop mode.
    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    /// Begin scrubbing mode.
    pub fn start_scrubbing(&mut self) {
        self.state = PlaybackState::Scrubbing;
    }

    /// Finish scrubbing mode (sets state to Paused).
    pub fn stop_scrubbing(&mut self) {
        self.state = PlaybackState::Paused;
    }

    /// Update position while in scrubbing mode.
    pub fn scrub_to(&mut self, time: TimeCode) {
        self.state = PlaybackState::Scrubbing;
        self.seek(time);
    }

    /// Update position in seconds while in scrubbing mode.
    pub fn scrub_to_seconds(&mut self, seconds: f64) {
        self.state = PlaybackState::Scrubbing;
        self.seek_seconds(seconds);
    }

    /// Update position to a discrete frame while in scrubbing mode.
    pub fn scrub_to_frame(&mut self, frame: i64) {
        self.state = PlaybackState::Scrubbing;
        self.seek_frame(frame);
    }

    // --- Seeking & Stepping ---

    /// Seek to exact discrete frame index. Clears subframe offset.
    pub fn seek_frame(&mut self, frame: i64) {
        self.current_frame = frame;
        self.position_nanos = self.frame_rate.frame_to_nanos(frame);
        self.subframe = 0.0;
    }

    /// Seek to continuous time in seconds. Recomputes quantized frame and fractional subframe.
    pub fn seek_seconds(&mut self, seconds: f64) {
        let nanos = (seconds * 1_000_000_000.0).round() as i128;
        self.position_nanos = nanos;
        self.update_frame_and_subframe();
    }

    /// Seek to given TimeCode.
    pub fn seek(&mut self, time: TimeCode) {
        self.seek_frame(time.frames());
    }

    /// Step forward by `frames` (pauses playback if playing).
    pub fn step_forward(&mut self, frames: i64) {
        self.pause();
        let max_frame = self.comp_duration.frames();
        let target = (self.current_frame + frames).min(max_frame);
        self.seek_frame(target);
    }

    /// Step backward by `frames` (pauses playback if playing).
    pub fn step_backward(&mut self, frames: i64) {
        self.pause();
        let min_frame = self.comp_start.frames();
        let target = (self.current_frame - frames).max(min_frame);
        self.seek_frame(target);
    }

    /// Step forward by 1 single frame.
    pub fn step_next_frame(&mut self) {
        self.step_forward(1);
    }

    /// Step backward by 1 single frame.
    pub fn step_prev_frame(&mut self) {
        self.step_backward(1);
    }

    /// Jump to the work area in-point.
    pub fn jump_to_start(&mut self) {
        self.seek_frame(self.work_area.in_point.frames());
    }

    /// Jump to the work area out-point.
    pub fn jump_to_end(&mut self) {
        self.seek_frame(self.work_area.out_point.frames());
    }

    /// Jump to the start of the composition (frame 0).
    pub fn jump_to_comp_start(&mut self) {
        self.seek_frame(self.comp_start.frames());
    }

    /// Jump to the end of the composition.
    pub fn jump_to_comp_end(&mut self) {
        self.seek_frame(self.comp_duration.frames());
    }

    // --- Keyframe & Marker Jumping ---

    /// Jump to the next timestamp strictly after the current frame in `times`.
    /// Returns `true` if a match was found and jumped to.
    pub fn jump_to_next_time(&mut self, times: &[TimeCode]) -> bool {
        if let Some(next) = times
            .iter()
            .filter(|t| t.frames() > self.current_frame)
            .min_by_key(|t| t.frames())
        {
            self.seek(*next);
            true
        } else {
            false
        }
    }

    /// Jump to the previous timestamp strictly before the current frame in `times`.
    /// Returns `true` if a match was found and jumped to.
    pub fn jump_to_previous_time(&mut self, times: &[TimeCode]) -> bool {
        if let Some(prev) = times
            .iter()
            .filter(|t| t.frames() < self.current_frame)
            .max_by_key(|t| t.frames())
        {
            self.seek(*prev);
            true
        } else {
            false
        }
    }

    /// Jump to the next marker strictly after the current frame.
    pub fn jump_to_next_marker(&mut self, markers: &[Marker]) -> bool {
        let times: Vec<TimeCode> = markers.iter().map(|m| m.time).collect();
        self.jump_to_next_time(&times)
    }

    /// Jump to the previous marker strictly before the current frame.
    pub fn jump_to_previous_marker(&mut self, markers: &[Marker]) -> bool {
        let times: Vec<TimeCode> = markers.iter().map(|m| m.time).collect();
        self.jump_to_previous_time(&times)
    }

    /// Jump to the next keyframe strictly after the current frame.
    pub fn jump_to_next_keyframe(&mut self, keyframes: &[TimeCode]) -> bool {
        self.jump_to_next_time(keyframes)
    }

    /// Jump to previous keyframe strictly before the current frame.
    pub fn jump_to_previous_keyframe(&mut self, keyframes: &[TimeCode]) -> bool {
        self.jump_to_previous_time(keyframes)
    }

    /// Jump to previous keyframe strictly before the current frame (alias).
    pub fn jump_to_prev_keyframe(&mut self, keyframes: &[TimeCode]) -> bool {
        self.jump_to_previous_keyframe(keyframes)
    }

    /// Jump to previous marker strictly before the current frame (alias).
    pub fn jump_to_prev_marker(&mut self, markers: &[Marker]) -> bool {
        self.jump_to_previous_marker(markers)
    }

    /// Jump to next keyframe across all animated properties in `comp`.
    pub fn jump_to_next_keyframe_in_comp(&mut self, comp: &Composition) -> bool {
        let times = comp.all_keyframe_times();
        self.jump_to_next_time(&times)
    }

    /// Jump to previous keyframe across all animated properties in `comp`.
    pub fn jump_to_prev_keyframe_in_comp(&mut self, comp: &Composition) -> bool {
        let times = comp.all_keyframe_times();
        self.jump_to_previous_time(&times)
    }

    /// Jump to previous keyframe across all animated properties in `comp` (alias).
    pub fn jump_to_previous_keyframe_in_comp(&mut self, comp: &Composition) -> bool {
        self.jump_to_prev_keyframe_in_comp(comp)
    }

    /// Jump to next marker on `comp` or its layers.
    pub fn jump_to_next_marker_in_comp(&mut self, comp: &Composition) -> bool {
        let times = comp.all_marker_times();
        self.jump_to_next_time(&times)
    }

    /// Jump to previous marker on `comp` or its layers.
    pub fn jump_to_prev_marker_in_comp(&mut self, comp: &Composition) -> bool {
        let times = comp.all_marker_times();
        self.jump_to_previous_time(&times)
    }

    /// Jump to previous marker on `comp` or its layers (alias).
    pub fn jump_to_previous_marker_in_comp(&mut self, comp: &Composition) -> bool {
        self.jump_to_prev_marker_in_comp(comp)
    }

    // --- Work Area Controls ---

    /// Set work area bounds. Validates `in_point <= out_point`.
    pub fn set_work_area(&mut self, in_point: TimeCode, out_point: TimeCode) -> Result<(), ValidationError> {
        let wa = WorkArea::new(in_point, out_point)?;
        self.work_area = wa;
        Ok(())
    }

    /// Set work area in-point.
    pub fn set_work_area_in(&mut self, in_point: TimeCode) {
        if in_point.frames() <= self.work_area.out_point.frames() {
            self.work_area.in_point = in_point;
        }
    }

    /// Set work area out-point.
    pub fn set_work_area_out(&mut self, out_point: TimeCode) {
        if out_point.frames() >= self.work_area.in_point.frames() {
            self.work_area.out_point = out_point;
        }
    }

    /// Reset work area to cover the entire composition timeline `[comp_start, comp_duration]`.
    pub fn reset_work_area_to_composition(&mut self) {
        self.work_area = WorkArea {
            in_point: self.comp_start,
            out_point: self.comp_duration,
        };
    }

    /// Set rational frame rate, resampling the current position in seconds.
    pub fn set_frame_rate(&mut self, frame_rate: FrameRate) {
        self.frame_rate = frame_rate;
        self.update_frame_and_subframe();
    }

    // --- High-Precision Update / Tick Engine ---

    /// Advance the transport by a delta time $\Delta t$.
    ///
    /// Accounts for playback speed, reverse direction, work area looping, Once pausing,
    /// and PingPong bouncing. Drift-free integer nanosecond accumulation guarantees zero
    /// clock drift over arbitrary running time.
    pub fn tick(&mut self, dt: Duration) -> ClockTickResult {
        if self.state != PlaybackState::Playing || dt.is_zero() || self.speed == 0.0 {
            return ClockTickResult {
                timecode: self.timecode(),
                frame: self.current_frame,
                frame_changed: false,
                looped: false,
                reached_end: false,
            };
        }

        let prev_frame = self.current_frame;
        let mut looped = false;
        let mut reached_end = false;

        // Compute delta nanoseconds based on continuous wall clock Duration and speed multiplier
        let dt_nanos = if (self.speed - 1.0).abs() < 1e-12 {
            dt.as_nanos() as i128
        } else if (self.speed - (-1.0)).abs() < 1e-12 {
            -(dt.as_nanos() as i128)
        } else {
            (dt.as_nanos() as f64 * self.speed).round() as i128
        };
        self.position_nanos += dt_nanos;

        let in_nanos = self.frame_rate.frame_to_nanos(self.work_area.in_point.frames());
        let out_nanos = self.frame_rate.frame_to_nanos(self.work_area.out_point.frames());
        let span_nanos = out_nanos - in_nanos;

        if span_nanos <= 0 {
            self.position_nanos = in_nanos;
            self.pause();
            reached_end = true;
        } else {
            match self.loop_mode {
                LoopMode::Once => {
                    if self.speed > 0.0 && self.position_nanos >= out_nanos {
                        self.position_nanos = out_nanos;
                        self.pause();
                        reached_end = true;
                    } else if self.speed < 0.0 && self.position_nanos <= in_nanos {
                        self.position_nanos = in_nanos;
                        self.pause();
                        reached_end = true;
                    }
                }
                LoopMode::Loop => {
                    if self.speed > 0.0 && self.position_nanos >= out_nanos {
                        let overflow = self.position_nanos - out_nanos;
                        self.position_nanos = in_nanos + (overflow % span_nanos);
                        looped = true;
                    } else if self.speed < 0.0 && self.position_nanos < in_nanos {
                        let underflow = in_nanos - self.position_nanos;
                        self.position_nanos = out_nanos - (underflow % span_nanos);
                        looped = true;
                    }
                }
                LoopMode::PingPong => {
                    // O(1) constant-time modular reflection handles any dt or span without loop latency
                    if self.position_nanos > out_nanos {
                        let overflow = self.position_nanos - out_nanos;
                        let period = 2 * span_nanos;
                        let rem = overflow % period;
                        if rem <= span_nanos {
                            self.position_nanos = out_nanos - rem;
                            self.speed = -self.speed.abs();
                            self.direction = PlaybackDirection::Reverse;
                        } else {
                            self.position_nanos = in_nanos + (rem - span_nanos);
                            self.speed = self.speed.abs();
                            self.direction = PlaybackDirection::Forward;
                        }
                        looped = true;
                    } else if self.position_nanos < in_nanos {
                        let underflow = in_nanos - self.position_nanos;
                        let period = 2 * span_nanos;
                        let rem = underflow % period;
                        if rem <= span_nanos {
                            self.position_nanos = in_nanos + rem;
                            self.speed = self.speed.abs();
                            self.direction = PlaybackDirection::Forward;
                        } else {
                            self.position_nanos = out_nanos - (rem - span_nanos);
                            self.speed = -self.speed.abs();
                            self.direction = PlaybackDirection::Reverse;
                        }
                        looped = true;
                    }
                }
            }
        }

        // Recompute quantized frame and fractional subframe from position_nanos
        self.update_frame_and_subframe();
        let frame_changed = self.current_frame != prev_frame;

        ClockTickResult {
            timecode: self.timecode(),
            frame: self.current_frame,
            frame_changed,
            looped,
            reached_end,
        }
    }

    /// Internal helper to recompute discrete frame and subframe from `position_nanos`.
    fn update_frame_and_subframe(&mut self) {
        let (frame, subframe) = self.frame_rate.nanos_to_frames(self.position_nanos);
        self.current_frame = frame;
        self.subframe = subframe;
    }
}

/// Ergonomic transport type alias.
pub type Transport = PlaybackClock;

impl Default for PlaybackClock {
    fn default() -> Self {
        Self::new(
            FrameRate::FPS_30,
            TimeCode::from_frames(300, 30.0), // 10 seconds at 30fps
        )
    }
}
