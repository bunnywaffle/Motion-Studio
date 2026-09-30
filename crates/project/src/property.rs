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

    /// Check whether a keyframe exists at the specified timecode (within 1e-5s).
    pub fn has_keyframe_at(&self, time: &TimeCode) -> bool {
        self.keyframe_at(time).is_some()
    }

    /// Return the timecode of the keyframe immediately preceding `time`, if any.
    pub fn previous_keyframe_time(&self, time: &TimeCode) -> Option<TimeCode> {
        let t = time.seconds();
        self.keyframes
            .iter()
            .rev()
            .find(|k| k.time_seconds() < t - 1e-5)
            .map(|k| k.time)
    }

    /// Return the timecode of the keyframe immediately following `time`, if any.
    pub fn next_keyframe_time(&self, time: &TimeCode) -> Option<TimeCode> {
        let t = time.seconds();
        self.keyframes
            .iter()
            .find(|k| k.time_seconds() > t + 1e-5)
            .map(|k| k.time)
    }

    /// Toggle a keyframe at the specified timecode:
    /// If a keyframe already exists, remove it and return `false`.
    /// Otherwise, add a keyframe with the provided value and return `true`.
    pub fn toggle_keyframe(&mut self, time: TimeCode, value: T) -> bool {
        if self.has_keyframe_at(&time) {
            self.remove_keyframe_at(&time);
            false
        } else {
            self.add_keyframe(Keyframe::new(time, value));
            true
        }
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

/// Deserialize a [`Property`] from either its full struct form or a bare
/// value (back-compat for project files saved before a field became
/// animatable). Bare values keep their value with a blank name.
pub fn de_property_or_value<'de, D, T>(deserializer: D) -> Result<Property<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Clone + Default,
{
    #[derive(serde::Deserialize)]
    #[serde(
        untagged,
        bound(deserialize = "T: serde::Deserialize<'de> + Clone + Default")
    )]
    enum PropOrVal<T> {
        Prop(Property<T>),
        Val(T),
    }
    Ok(match PropOrVal::deserialize(deserializer)? {
        PropOrVal::Prop(p) => p,
        PropOrVal::Val(v) => Property::new(String::new(), v),
    })
}

/// Deserialize an `Option<Property>` from null, a bare value, or a full
/// property struct (back-compat for fields that became animatable).
pub fn de_opt_property_or_value<'de, D, T>(
    deserializer: D,
) -> Result<Option<Property<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Clone + Default,
{
    #[derive(serde::Deserialize)]
    #[serde(
        untagged,
        bound(deserialize = "T: serde::Deserialize<'de> + Clone + Default")
    )]
    enum OptPropOrVal<T> {
        None,
        Prop(Property<T>),
        Val(T),
    }
    // Untagged tries in order: None matches null.
    Ok(match OptPropOrVal::deserialize(deserializer)? {
        OptPropOrVal::None => None,
        OptPropOrVal::Prop(p) => Some(p),
        OptPropOrVal::Val(v) => Some(Property::new(String::new(), v)),
    })
}

/// Deserialize a `Vec<Property<T>>` from either full property structs or
/// bare values (back-compat for slots saved before they became animatable).
pub fn de_vec_property_or_value<'de, D, T>(
    deserializer: D,
) -> Result<Vec<Property<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Clone + Default,
{
    #[derive(serde::Deserialize)]
    #[serde(
        untagged,
        bound(deserialize = "T: serde::Deserialize<'de> + Clone + Default")
    )]
    enum VecPropOrVal<T> {
        Props(Vec<Property<T>>),
        Vals(Vec<T>),
    }
    Ok(match VecPropOrVal::deserialize(deserializer)? {
        VecPropOrVal::Props(p) => p,
        VecPropOrVal::Vals(v) => v
            .into_iter()
            .map(|t| Property::new(String::new(), t))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn bare_value_migrates_to_property() {
        #[derive(Debug, PartialEq, serde::Deserialize)]
        struct Probe {
            #[serde(deserialize_with = "crate::property::de_property_or_value")]
            #[serde(default)]
            color: Property<Color>,
            #[serde(
                default,
                skip_serializing_if = "Option::is_none",
                deserialize_with = "crate::property::de_opt_property_or_value"
            )]
            maybe: Option<Property<Color>>,
        }
        // Bare legacy values.
        let p: Probe = serde_json::from_str(
            r#"{"color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}}"#,
        )
        .unwrap();
        assert_eq!(p.color.value, Color::RED);
        assert_eq!(p.maybe, None);
        // Full property form survives with keyframes.
        let p: Probe = serde_json::from_str(
            r#"{"color": {"name": "C", "value": {"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0}, "default_value": {"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0}, "animated": true, "keyframes": []}, "maybe": null}"#,
        )
        .unwrap();
        assert!(p.color.is_animated());
        assert_eq!(p.maybe, None);
    }

    #[test]
    fn bare_vec_values_migrate_to_properties() {
        #[derive(Debug, PartialEq, serde::Deserialize)]
        struct VecProbe {
            #[serde(deserialize_with = "crate::property::de_vec_property_or_value")]
            #[serde(default)]
            colors: Vec<Property<Color>>,
        }
        // Bare legacy slot colors.
        let p: VecProbe = serde_json::from_str(
            r#"{"colors": [{"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}]}"#,
        )
        .unwrap();
        assert_eq!(p.colors.len(), 1);
        assert_eq!(p.colors[0].value, Color::RED);
        // Full property form survives.
        let p: VecProbe = serde_json::from_str(
            r#"{"colors": [{"name": "color", "value": {"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0}, "default_value": {"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0}}]}"#,
        )
        .unwrap();
        assert_eq!(p.colors[0].value.b, 1.0);
        assert_eq!(p.colors[0].name, "color");
    }
}
