use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

/// A generic property wrapper providing default value tracking, animation flags,
/// and uniform naming for animatable motion graphics attributes (transforms, opacity, colors, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property<T> {
    pub name: String,
    pub value: T,
    pub default_value: T,
    #[serde(default)]
    pub animated: bool,
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
        }
    }

    /// Create a new property with explicitly distinct initial and default values.
    pub fn with_default(name: impl Into<String>, value: T, default_value: T) -> Self {
        Self {
            name: name.into(),
            value,
            default_value,
            animated: false,
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
        self.value == self.default_value
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
