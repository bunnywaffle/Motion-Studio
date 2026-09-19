use crate::keyframe::{evaluate_keyframe_track, Extrapolation, Interpolate, Keyframe};
use crate::timecode::TimeCode;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

/// A generic property wrapper providing default value tracking, animation flags,
/// keyframe track management, and uniform naming for animatable motion graphics attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property<T> {
    pub name: String,
    pub value: T,
    pub default_value: T,
    #[serde(default)]
    pub animated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyframes: Vec<Keyframe<T>>,
}

impl<T> Property<T> {
    /// Create a new property where the current value and default value are initialized to `value`.
    pub fn new(name: impl Into<String>, value: T) -> Self
    where
        T: Clone,
    {
        Self {
            name: name.into(),
            default_value: value.clone(),
            value,
            animated: false,
            keyframes: Vec::new(),
        }
    }

    /// Create a new property with explicitly distinct initial and default values.
    pub fn with_default(name: impl Into<String>, value: T, default_value: T) -> Self {
        Self {
            name: name.into(),
            value,
            default_value,
            animated: false,
            keyframes: Vec::new(),
        }
    }

    /// Builder method to set the `animated` flag.
    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }

    /// Create a property with initial and default value set to `T::default()`.
    pub fn new_default(name: impl Into<String>) -> Self
    where
        T: Default + Clone,
    {
        let def = T::default();
        Self {
            name: name.into(),
            default_value: def.clone(),
            value: def,
            animated: false,
            keyframes: Vec::new(),
        }
    }

    /// Get a reference to the current property value.
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Get a mutable reference to the current property value.
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// Get a reference to the default property value.
    pub const fn default_value(&self) -> &T {
        &self.default_value
    }

    /// Set a new current value.
    pub fn set_value(&mut self, value: T) {
        self.value = value;
    }

    /// Set a new default value.
    pub fn set_default_value(&mut self, default_value: T) {
        self.default_value = default_value;
    }

    /// Get the property name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check whether the property is currently animated.
    pub const fn is_animated(&self) -> bool {
        self.animated
    }

    /// Set the animation status.
    pub fn set_animated(&mut self, animated: bool) {
        self.animated = animated;
    }

    /// Reset the property value back to its default value.
    pub fn reset(&mut self)
    where
        T: Clone,
    {
        self.value = self.default_value.clone();
    }

    /// Check whether the current value equals the default value.
    pub fn is_default(&self) -> bool
    where
        T: PartialEq,
    {
        self.value == self.default_value && self.keyframes.is_empty()
    }

    /// Return an immutable slice of all keyframes.
    pub fn keyframes(&self) -> &[Keyframe<T>] {
        &self.keyframes
    }

    /// Return a mutable reference to the keyframes vector.
    pub fn keyframes_mut(&mut self) -> &mut Vec<Keyframe<T>> {
        &mut self.keyframes
    }

    /// Check whether this property contains any keyframes.
    pub fn has_keyframes(&self) -> bool {
        !self.keyframes.is_empty()
    }

    /// Return the count of keyframes on this property.
    pub fn keyframe_count(&self) -> usize {
        self.keyframes.len()
    }

    /// Clear all keyframes and disable the animated flag.
    pub fn clear_keyframes(&mut self) {
        self.keyframes.clear();
        self.animated = false;
    }

    /// Add or update a keyframe, automatically maintaining temporal sort order.
    pub fn add_keyframe(&mut self, keyframe: Keyframe<T>) {
        let t = keyframe.time_seconds();
        let idx = self.keyframes.partition_point(|k| k.time_seconds() < t);
        if idx < self.keyframes.len() && (self.keyframes[idx].time_seconds() - t).abs() < 1e-6 {
            self.keyframes[idx] = keyframe;
        } else {
            self.keyframes.insert(idx, keyframe);
        }
        self.animated = true;
    }

    /// Remove a keyframe matching the specified timecode (within 1e-5s). Returns removed keyframe if found.
    pub fn remove_keyframe_at(&mut self, time: &TimeCode) -> Option<Keyframe<T>> {
        let t = time.seconds();
        if let Some(idx) = self.keyframes.iter().position(|k| (k.time_seconds() - t).abs() < 1e-5) {
            let removed = self.keyframes.remove(idx);
            if self.keyframes.is_empty() {
                self.animated = false;
            }
            Some(removed)
        } else {
            None
        }
    }

    /// Find a keyframe at or closest to the given timecode.
    pub fn keyframe_at(&self, time: &TimeCode) -> Option<&Keyframe<T>> {
        let t = time.seconds();
        self.keyframes.iter().find(|k| (k.time_seconds() - t).abs() < 1e-5)
    }
}

impl<T: Interpolate> Property<T> {
    /// Evaluate the property at a specific TimeCode using standard boundary holding.
    pub fn evaluate_at(&self, time: &TimeCode) -> T {
        self.evaluate_at_seconds(time.seconds())
    }

    /// Evaluate the property at arbitrary floating-point seconds using standard boundary holding.
    pub fn evaluate_at_seconds(&self, seconds: f64) -> T {
        self.evaluate_with_extrapolation(seconds, Extrapolation::Hold, Extrapolation::Hold)
    }

    /// Evaluate the property at arbitrary floating-point seconds with explicit pre- and post-extrapolation modes.
    pub fn evaluate_with_extrapolation(
        &self,
        seconds: f64,
        pre: Extrapolation,
        post: Extrapolation,
    ) -> T {
        if !self.animated || self.keyframes.is_empty() {
            return self.value.clone();
        }
        evaluate_keyframe_track(&self.keyframes, seconds, &self.value, pre, post)
    }
}

impl<T: Default + Clone> Default for Property<T> {
    fn default() -> Self {
        Self::new_default(String::new())
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Property<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.value)
    }
}

impl<T> Deref for Property<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for Property<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}
