use serde::{Deserialize, Serialize};
use std::fmt;

/// Track matte compositing modes for layer masking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackMatteMode {
    /// No track matte applied.
    #[default]
    None,
    /// Use the alpha channel of the matte layer.
    Alpha,
    /// Use the inverted alpha channel of the matte layer.
    AlphaInverted,
    /// Use the luminance/brightness values of the matte layer.
    Luma,
    /// Use the inverted luminance values of the matte layer.
    LumaInverted,
}

impl TrackMatteMode {
    /// Return true if this mode activates track matte processing.
    pub const fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Return true if this is an alpha-based matte mode.
    pub const fn is_alpha(&self) -> bool {
        matches!(self, Self::Alpha | Self::AlphaInverted)
    }

    /// Return true if this is a luminance-based matte mode.
    pub const fn is_luma(&self) -> bool {
        matches!(self, Self::Luma | Self::LumaInverted)
    }

    /// Return true if this matte mode inverts the matte signal.
    pub const fn is_inverted(&self) -> bool {
        matches!(self, Self::AlphaInverted | Self::LumaInverted)
    }

    /// Return the canonical display label.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Alpha => "Alpha Matte",
            Self::AlphaInverted => "Alpha Inverted Matte",
            Self::Luma => "Luma Matte",
            Self::LumaInverted => "Luma Inverted Matte",
        }
    }
}

impl fmt::Display for TrackMatteMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
