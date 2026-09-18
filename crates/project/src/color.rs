use crate::error::ColorError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// An RGBA color representation using 32-bit floating point components in the range [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self::rgba(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::rgba(1.0, 1.0, 1.0, 1.0);
    pub const RED: Self = Self::rgba(1.0, 0.0, 0.0, 1.0);
    pub const GREEN: Self = Self::rgba(0.0, 1.0, 0.0, 1.0);
    pub const BLUE: Self = Self::rgba(0.0, 0.0, 1.0, 1.0);
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);

    /// Create a color from floating point RGBA values, clamped to [0.0, 1.0].
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: clamp_01(r),
            g: clamp_01(g),
            b: clamp_01(b),
            a: clamp_01(a),
        }
    }

    /// Create an opaque color from floating point RGB values (alpha = 1.0).
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// Create a color from 8-bit per channel RGBA values (0..255).
    pub fn from_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    /// Create an opaque color from 8-bit per channel RGB values (0..255).
    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgba_u8(r, g, b, 255)
    }

    /// Parse a hexadecimal color string (supports `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`).
    pub fn from_hex(hex: &str) -> Result<Self, ColorError> {
        let clean = hex.trim().strip_prefix('#').unwrap_or(hex.trim());

        // Validate that all characters are hexadecimal digits
        for c in clean.chars() {
            if !c.is_ascii_hexdigit() {
                return Err(ColorError::InvalidHexDigit(c));
            }
        }

        let parse_nibble = |c: char| -> Result<u8, ColorError> {
            c.to_digit(16)
                .map(|d| d as u8)
                .ok_or(ColorError::InvalidHexDigit(c))
        };

        match clean.len() {
            // #RGB -> #RRGGBB
            3 => {
                let mut chars = clean.chars();
                let r = parse_nibble(chars.next().unwrap())?;
                let g = parse_nibble(chars.next().unwrap())?;
                let b = parse_nibble(chars.next().unwrap())?;
                Ok(Self::from_rgb_u8((r << 4) | r, (g << 4) | g, (b << 4) | b))
            }
            // #RGBA -> #RRGGBBAA
            4 => {
                let mut chars = clean.chars();
                let r = parse_nibble(chars.next().unwrap())?;
                let g = parse_nibble(chars.next().unwrap())?;
                let b = parse_nibble(chars.next().unwrap())?;
                let a = parse_nibble(chars.next().unwrap())?;
                Ok(Self::from_rgba_u8(
                    (r << 4) | r,
                    (g << 4) | g,
                    (b << 4) | b,
                    (a << 4) | a,
                ))
            }
            // #RRGGBB
            6 => {
                let r = u8::from_str_radix(&clean[0..2], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                let g = u8::from_str_radix(&clean[2..4], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                let b = u8::from_str_radix(&clean[4..6], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                Ok(Self::from_rgb_u8(r, g, b))
            }
            // #RRGGBBAA
            8 => {
                let r = u8::from_str_radix(&clean[0..2], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                let g = u8::from_str_radix(&clean[2..4], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                let b = u8::from_str_radix(&clean[4..6], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                let a = u8::from_str_radix(&clean[6..8], 16)
                    .map_err(|_| ColorError::InvalidHexFormat(hex.to_string()))?;
                Ok(Self::from_rgba_u8(r, g, b, a))
            }
            _ => Err(ColorError::InvalidHexFormat(hex.to_string())),
        }
    }

    /// Convert to 8-bit per channel `(r, g, b, a)` values (0..255).
    pub fn to_rgba_u8(&self) -> (u8, u8, u8, u8) {
        (
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
            (self.a * 255.0).round() as u8,
        )
    }

    /// Convert to `#RRGGBB` format.
    pub fn to_hex_rgb(&self) -> String {
        let (r, g, b, _) = self.to_rgba_u8();
        format!("#{r:02X}{g:02X}{b:02X}")
    }

    /// Convert to `#RRGGBBAA` format.
    pub fn to_hex_rgba(&self) -> String {
        let (r, g, b, a) = self.to_rgba_u8();
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

const fn clamp_01(v: f32) -> f32 {
    if v.is_nan() || v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex_rgba())
    }
}
