use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Classification of media and project asset files.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Image,
    Video,
    Audio,
    Font,
    Vector,
    Other(String),
}

impl AssetType {
    /// Infer the asset type from a file path based on its extension.
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tiff" | "tga" | "exr" | "hdr" => {
                Self::Image
            }
            "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" | "flv" | "wmv" => Self::Video,
            "wav" | "mp3" | "aac" | "ogg" | "flac" | "m4a" | "aiff" => Self::Audio,
            "ttf" | "otf" | "woff" | "woff2" => Self::Font,
            "svg" | "ai" | "eps" => Self::Vector,
            other => {
                if other.is_empty() {
                    Self::Other("unknown".to_string())
                } else {
                    Self::Other(other.to_string())
                }
            }
        }
    }
}

/// An external or imported media asset referenced by the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub asset_type: AssetType,
}

impl Asset {
    /// Create a new asset with an explicit asset type.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        asset_type: AssetType,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            asset_type,
        }
    }

    /// Create a new asset, automatically detecting the asset type from the file path extension.
    pub fn from_path(
        id: impl Into<String>,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        let path_buf = path.into();
        let asset_type = AssetType::from_path(&path_buf);
        Self {
            id: id.into(),
            name: name.into(),
            path: path_buf,
            asset_type,
        }
    }

    /// Return true if this asset is an image.
    pub const fn is_image(&self) -> bool {
        matches!(self.asset_type, AssetType::Image)
    }

    /// Return true if this asset is a video.
    pub const fn is_video(&self) -> bool {
        matches!(self.asset_type, AssetType::Video)
    }

    /// Return true if this asset is an audio track.
    pub const fn is_audio(&self) -> bool {
        matches!(self.asset_type, AssetType::Audio)
    }

    /// Return true if this asset is a font file.
    pub const fn is_font(&self) -> bool {
        matches!(self.asset_type, AssetType::Font)
    }

    /// Return true if this asset is a vector graphic.
    pub const fn is_vector(&self) -> bool {
        matches!(self.asset_type, AssetType::Vector)
    }
}
