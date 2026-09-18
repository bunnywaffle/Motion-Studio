use crate::asset::Asset;
use crate::composition::Composition;
use crate::error::{ProjectError, ValidationError};
use crate::layer::{Layer, LayerSource};
use crate::timecode::TimeCode;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;

/// The current project file format version.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

/// Project-wide environment and render settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub working_color_space: String,
    pub audio_sample_rate: u32,
    pub start_timecode: TimeCode,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            working_color_space: "sRGB".to_string(),
            audio_sample_rate: 48_000,
            start_timecode: TimeCode::from_frames(0, 30.0),
        }
    }
}

/// The top-level project model holding versioning, metadata, compositions, assets, and project settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    pub compositions: Vec<Composition>,
    pub assets: Vec<Asset>,
    pub settings: ProjectSettings,
}

impl Project {
    /// Create a new project with explicit ID and name, initialized with default settings and empty collections.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            id: id.into(),
            name: name.into(),
            compositions: Vec::new(),
            assets: Vec::new(),
            settings: ProjectSettings::default(),
        }
    }

    /// Convenience constructor generating a project with an automated ID and default settings.
    pub fn with_defaults(name: impl Into<String>) -> Self {
        let name_str = name.into();
        let id = format!("proj_{}", name_str.to_lowercase().replace(' ', "_"));
        Self::new(id, name_str)
    }

    // --- Composition Management ---

    /// Add a composition to the project.
    pub fn add_composition(&mut self, composition: Composition) -> Result<(), ValidationError> {
        if self.compositions.iter().any(|c| c.id == composition.id) {
            return Err(ValidationError::DuplicateCompositionId(composition.id));
        }
        self.compositions.push(composition);
        Ok(())
    }

    /// Get an immutable reference to a composition by ID.
    pub fn get_composition(&self, id: &str) -> Option<&Composition> {
        self.compositions.iter().find(|c| c.id == id)
    }

    /// Get a mutable reference to a composition by ID.
    pub fn get_composition_mut(&mut self, id: &str) -> Option<&mut Composition> {
        self.compositions.iter_mut().find(|c| c.id == id)
    }

    /// Remove a composition by ID.
    pub fn remove_composition(&mut self, id: &str) -> Option<Composition> {
        if let Some(idx) = self.compositions.iter().position(|c| c.id == id) {
            Some(self.compositions.remove(idx))
        } else {
            None
        }
    }

    // --- Asset Management & Tracking ---

    /// Add an asset to the project.
    pub fn add_asset(&mut self, asset: Asset) -> Result<(), ValidationError> {
        if self.assets.iter().any(|a| a.id == asset.id) {
            return Err(ValidationError::DuplicateAssetId(asset.id));
        }
        self.assets.push(asset);
        Ok(())
    }

    /// Get an immutable reference to an asset by ID.
    pub fn get_asset(&self, id: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Get a mutable reference to an asset by ID.
    pub fn get_asset_mut(&mut self, id: &str) -> Option<&mut Asset> {
        self.assets.iter_mut().find(|a| a.id == id)
    }

    /// Remove an asset by ID.
    pub fn remove_asset(&mut self, id: &str) -> Option<Asset> {
        if let Some(idx) = self.assets.iter().position(|a| a.id == id) {
            Some(self.assets.remove(idx))
        } else {
            None
        }
    }

    /// Find all layers across all compositions in the project that reference a specific asset ID.
    pub fn find_layers_referencing_asset(&self, asset_id: &str) -> Vec<(&Composition, &Layer)> {
        let mut results = Vec::new();
        for comp in &self.compositions {
            for layer in &comp.layers {
                if layer.referenced_asset_id() == Some(asset_id) {
                    results.push((comp, layer));
                }
            }
        }
        results
    }

    /// Find all compositions and layers that reference a given nested composition ID.
    pub fn find_compositions_referencing_composition(
        &self,
        comp_id: &str,
    ) -> Vec<(&Composition, &Layer)> {
        let mut results = Vec::new();
        for comp in &self.compositions {
            for layer in &comp.layers {
                if layer.referenced_composition_id() == Some(comp_id) {
                    results.push((comp, layer));
                }
            }
        }
        results
    }

    /// Return the total number of layers across all compositions in the project.
    pub fn total_layers_count(&self) -> usize {
        self.compositions.iter().map(|c| c.layers.len()).sum()
    }

    /// Find a layer and its parent composition by layer ID across all compositions.
    pub fn find_layer(&self, layer_id: &str) -> Option<(&Composition, &Layer)> {
        for comp in &self.compositions {
            if let Some(layer) = comp.get_layer(layer_id) {
                return Some((comp, layer));
            }
        }
        None
    }

    /// Find a mutable reference to a layer by layer ID across all compositions in the project.
    pub fn find_layer_mut(&mut self, layer_id: &str) -> Option<&mut Layer> {
        for comp in &mut self.compositions {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                return Some(layer);
            }
        }
        None
    }

    // --- Project Validation ---

    /// Perform full project-wide validation:
    /// - Individual composition structural validation.
    /// - Asset reference validation (every asset referenced by an image/video layer must exist in `self.assets`).
    /// - Nested composition existence validation.
    /// - Cycle detection for nested compositions (e.g. A -> B -> A).
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut comp_ids = HashSet::new();
        for comp in &self.compositions {
            if !comp_ids.insert(&comp.id) {
                return Err(ValidationError::DuplicateCompositionId(comp.id.clone()));
            }
            comp.validate()?;
        }

        let mut asset_ids = HashSet::new();
        for asset in &self.assets {
            if !asset_ids.insert(&asset.id) {
                return Err(ValidationError::DuplicateAssetId(asset.id.clone()));
            }
        }

        // Validate cross-references: assets and nested compositions
        for comp in &self.compositions {
            for layer in &comp.layers {
                match &layer.source {
                    LayerSource::Image { asset_id } | LayerSource::Video { asset_id, .. } => {
                        if !asset_ids.contains(asset_id) {
                            return Err(ValidationError::AssetNotFound(asset_id.clone()));
                        }
                    }
                    LayerSource::NestedComposition { composition_id }
                        if !comp_ids.contains(composition_id) =>
                    {
                        return Err(ValidationError::NestedCompositionNotFound {
                            layer_id: layer.id.clone(),
                            composition_id: composition_id.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        // Check for recursive nested composition cycles
        self.validate_nested_composition_cycles()?;

        Ok(())
    }

    fn validate_nested_composition_cycles(&self) -> Result<(), ValidationError> {
        for comp in &self.compositions {
            let mut visited = HashSet::new();
            visited.insert(comp.id.clone());
            self.detect_nested_cycle(comp, &mut visited, vec![comp.id.clone()])?;
        }
        Ok(())
    }

    fn detect_nested_cycle(
        &self,
        current: &Composition,
        visited: &mut HashSet<String>,
        path: Vec<String>,
    ) -> Result<(), ValidationError> {
        for layer in &current.layers {
            if let Some(nested_id) = layer.referenced_composition_id() {
                let mut current_path = path.clone();
                current_path.push(nested_id.to_string());
                if visited.contains(nested_id) {
                    return Err(ValidationError::CircularNestedComposition {
                        composition_id: current.id.clone(),
                        cycle: current_path,
                    });
                }
                if let Some(nested_comp) = self.get_composition(nested_id) {
                    visited.insert(nested_id.to_string());
                    self.detect_nested_cycle(nested_comp, visited, current_path)?;
                    visited.remove(nested_id);
                }
            }
        }
        Ok(())
    }

    // --- JSON Serialization & File I/O ---

    /// Serialize the project to a compact JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize the project to a formatted, human-readable JSON string.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a project from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Save the project to a JSON file at the specified path.
    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<(), ProjectError> {
        let json = self.to_json_pretty()?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a project from a JSON file at the specified path.
    pub fn load_from_file(path: impl AsRef<std::path::Path>) -> Result<Self, ProjectError> {
        let content = std::fs::read_to_string(path)?;
        let project = Self::from_json(&content)?;
        Ok(project)
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::with_defaults("Untitled Project")
    }
}

// Internal serialization representation conforming to Section 16 format
#[derive(Serialize)]
struct ProjectSerializedRef<'a> {
    format_version: u32,
    project: ProjectMetaRef<'a>,
    compositions: &'a [Composition],
    assets: &'a [Asset],
    settings: &'a ProjectSettings,
}

#[derive(Serialize)]
struct ProjectMetaRef<'a> {
    id: &'a str,
    name: &'a str,
}

impl Serialize for Project {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ProjectSerializedRef {
            format_version: self.format_version,
            project: ProjectMetaRef {
                id: &self.id,
                name: &self.name,
            },
            compositions: &self.compositions,
            assets: &self.assets,
            settings: &self.settings,
        }
        .serialize(serializer)
    }
}

// Internal deserialization helper supporting Section 16 format and legacy flat format
#[derive(Deserialize)]
struct ProjectDeserializedHelper {
    format_version: Option<u32>,
    project: Option<ProjectMetaHelper>,
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    compositions: Vec<Composition>,
    #[serde(default)]
    assets: Vec<Asset>,
    #[serde(default)]
    settings: Option<ProjectSettings>,
}

#[derive(Deserialize)]
struct ProjectMetaHelper {
    id: Option<String>,
    name: Option<String>,
}

impl<'de> Deserialize<'de> for Project {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let helper = ProjectDeserializedHelper::deserialize(deserializer)?;
        let format_version = helper.format_version.unwrap_or(CURRENT_FORMAT_VERSION);

        let (id, name) = if let Some(meta) = helper.project {
            let id = meta
                .id
                .or(helper.id)
                .unwrap_or_else(|| "untitled_project".to_string());
            let name = meta
                .name
                .or(helper.name)
                .unwrap_or_else(|| "Untitled Project".to_string());
            (id, name)
        } else {
            let id = helper.id.unwrap_or_else(|| "untitled_project".to_string());
            let name = helper.name.unwrap_or_else(|| "Untitled Project".to_string());
            (id, name)
        };

        let settings = helper.settings.unwrap_or_default();

        Ok(Self {
            format_version,
            id,
            name,
            compositions: helper.compositions,
            assets: helper.assets,
            settings,
        })
    }
}
