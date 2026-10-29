//! User-installed effect packages and the OpenFX bundle registry.
//!
//! An effect package is one Shader Lab source file: `// @meta name="..."`
//! (plus optional `category` / `description`) and a
//! `vec4 effect(vec2 uv, vec4 color)` entry point. Installing copies it
//! into the app's effects folder. Applying an installed effect embeds the
//! source in the project, so deleting the package never breaks a saved
//! project.
//!
//! OpenFX bundles are only *registered* here (name / identifier / path).
//! This host has no OFX runtime yet, so the UI must keep labelling them
//! unsupported — never imply their binaries execute.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shader::parse_shader_meta;

/// Extensions accepted by "Install Effect…" and scanned in the effects folder.
pub const EFFECT_EXTENSIONS: &[&str] = &["fx", "wgsl", "glsl", "txt"];

/// A validated effect package found in the app effects folder.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledEffect {
    /// File name inside the effects folder (the registry key).
    pub file: String,
    pub name: String,
    pub category: String,
    pub description: String,
    /// Package body, embedded into the project when applied.
    pub source: String,
    pub enabled: bool,
}

/// A registered third-party OpenFX bundle (metadata only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfxBundle {
    pub name: String,
    pub identifier: String,
    pub path: String,
}

/// On-disk plug-in state (`<config>/MotionStudio/plugins.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginRegistry {
    /// File names of installed effects switched off in the manager.
    #[serde(default)]
    pub disabled_effects: BTreeSet<String>,
    #[serde(default)]
    pub ofx_bundles: Vec<OfxBundle>,
}

/// Header fields parsed from an effect package.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectPackageMeta {
    pub name: String,
    pub category: String,
    pub description: String,
}

/// Parse the `@meta` header of an effect package. Returns `None` unless the
/// file has both a name and the Shader Lab entry point.
pub fn effect_meta(source: &str) -> Option<EffectPackageMeta> {
    if !source.contains("vec4 effect(") {
        return None;
    }
    let meta = parse_shader_meta(source);
    let name = meta.get("name")?.trim();
    if name.is_empty() {
        return None;
    }
    Some(EffectPackageMeta {
        name: name.to_string(),
        category: meta
            .get("category")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("Custom")
            .to_string(),
        description: meta
            .get("description")
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    })
}

/// Filesystem-safe stem for a package name (`Test Glow` → `test-glow`).
pub fn effect_slug(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "effect".to_string()
    } else {
        slug.to_string()
    }
}

/// Install a source into `dir` as `<slug>.fx`, replacing an earlier package
/// with the same name. Validation of the GLSL/WGSL itself is the caller's job
/// (it owns the compiler).
pub fn install_effect_source(source: &str, dir: &Path) -> Result<InstalledEffect, String> {
    let meta = effect_meta(source).ok_or_else(|| {
        "Not an effect package: add `// @meta name=\"...\"` and a \
         `vec4 effect(vec2 uv, vec4 color)` function"
            .to_string()
    })?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Create effects folder failed: {e}"))?;
    let file = format!("{}.fx", effect_slug(&meta.name));
    std::fs::write(dir.join(&file), source).map_err(|e| format!("Write effect failed: {e}"))?;
    Ok(InstalledEffect {
        file,
        name: meta.name,
        category: meta.category,
        description: meta.description,
        source: source.to_string(),
        enabled: true,
    })
}

/// Read and install one effect package file.
pub fn install_effect_file(src: &Path, dir: &Path) -> Result<InstalledEffect, String> {
    let source = std::fs::read_to_string(src).map_err(|e| format!("Read effect failed: {e}"))?;
    install_effect_source(&source, dir)
}

fn is_effect_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EFFECT_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Scan `dir` for valid effect packages, sorted by display name.
pub fn load_installed_effects(dir: &Path, registry: &PluginRegistry) -> Vec<InstalledEffect> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_effect_file(&path) {
            continue;
        }
        let Some(file) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(meta) = effect_meta(&source) else {
            continue;
        };
        out.push(InstalledEffect {
            enabled: !registry.disabled_effects.contains(&file),
            file,
            name: meta.name,
            category: meta.category,
            description: meta.description,
            source,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Delete an installed package. `file` must be a bare file name.
pub fn remove_effect_file(dir: &Path, file: &str) -> Result<(), String> {
    if Path::new(file).components().count() != 1 {
        return Err("Refusing to delete outside the effects folder".to_string());
    }
    std::fs::remove_file(dir.join(file)).map_err(|e| format!("Delete effect failed: {e}"))
}

/// Read the plug-in registry, tolerating a missing or corrupt file.
pub fn load_registry(path: &Path) -> PluginRegistry {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist the plug-in registry (creates parent directories).
pub fn save_registry(path: &Path, registry: &PluginRegistry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Create config folder failed: {e}"))?;
    }
    let text = serde_json::to_string_pretty(registry)
        .map_err(|e| format!("Serialize plug-in registry failed: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("Write plug-in registry failed: {e}"))
}

/// Recursively find `.ofx` bundles under `root` (depth-limited, stops at the
/// bundle itself). Missing roots yield an empty list.
pub fn find_ofx_bundles(root: &Path) -> Vec<OfxBundle> {
    let mut out = Vec::new();
    collect_ofx(root, 0, &mut out);
    out.sort_by_key(|b| b.name.to_lowercase());
    out.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));
    out
}

fn collect_ofx(dir: &Path, depth: usize, out: &mut Vec<OfxBundle>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.to_ascii_lowercase().ends_with(".ofx") {
            out.push(ofx_bundle(&path, &file_name));
            continue;
        }
        if path.is_dir() {
            collect_ofx(&path, depth + 1, out);
        }
    }
}

fn ofx_bundle(path: &Path, file_name: &str) -> OfxBundle {
    let name = plist_string(path, "CFBundleName")
        .or_else(|| plist_string(path, "CFBundleDisplayName"))
        .unwrap_or_else(|| {
            PathBuf::from(file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "OpenFX Bundle".to_string())
        });
    let identifier = plist_string(path, "CFBundleIdentifier")
        .unwrap_or_else(|| format!("local.{}", effect_slug(&name)));
    OfxBundle {
        name,
        identifier,
        path: path.to_string_lossy().into_owned(),
    }
}

/// Read one `<key>…</key><string>…</string>` value from a bundle's
/// `Contents/Info.plist` (bundles without one fall back to folder names).
fn plist_string(bundle: &Path, key: &str) -> Option<String> {
    let plist = bundle.join("Contents").join("Info.plist");
    let text = std::fs::read_to_string(plist).ok()?;
    let key_tag = format!("<key>{key}</key>");
    let after = &text[text.find(&key_tag)? + key_tag.len()..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    let value = after[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = r#"// @meta name="Test Glow" category="Stylize" description="Adds a test glow"
// @param name="Amount" type=float min=0 max=1 step=0.01
uniform float amount = 0.5;
vec4 effect(vec2 uv, vec4 color) { return color + vec4(amount * 0.1); }
"#;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "motion_plugin_{tag}_{}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_scan_disable_remove_effect_package() {
        let root = temp_dir("effects");
        let effects = root.join("effects");
        let src = root.join("source.txt");
        std::fs::write(&src, PACKAGE).unwrap();

        let installed = install_effect_file(&src, &effects).unwrap();
        assert_eq!(installed.name, "Test Glow");
        assert_eq!(installed.file, "test-glow.fx");
        assert_eq!(installed.category, "Stylize");
        assert!(installed.enabled);

        let mut registry = PluginRegistry::default();
        let listed = load_installed_effects(&effects, &registry);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].enabled);
        assert!(listed[0].source.contains("vec4 effect("));

        registry.disabled_effects.insert(installed.file.clone());
        assert!(!load_installed_effects(&effects, &registry)[0].enabled);

        remove_effect_file(&effects, &installed.file).unwrap();
        assert!(load_installed_effects(&effects, &registry).is_empty());
        assert!(remove_effect_file(&effects, &installed.file).is_err());
        assert!(remove_effect_file(&effects, "../escape.fx").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn effect_package_requires_name_and_entry_point() {
        assert!(
            effect_meta("uniform float x = 1.0;\nvec4 effect(vec2 uv, vec4 color) { return color; }")
                .is_none()
        );
        assert!(effect_meta("// @meta name=\"No Entry\"\nuniform float x = 1.0;").is_none());
        let meta = effect_meta(PACKAGE).unwrap();
        assert_eq!(meta.category, "Stylize");
        assert_eq!(effect_slug("  Foo / Bar!! "), "foo-bar");
        assert_eq!(effect_slug("!!!"), "effect");
    }

    #[test]
    fn registry_round_trip_and_missing_or_corrupt_file() {
        let root = temp_dir("registry");
        let path = root.join("plugins.json");
        assert_eq!(load_registry(&path), PluginRegistry::default());

        let mut registry = PluginRegistry::default();
        registry.disabled_effects.insert("a.fx".to_string());
        registry.ofx_bundles.push(OfxBundle {
            name: "X".to_string(),
            identifier: "com.example.x".to_string(),
            path: "C:/plugins/X.ofx".to_string(),
        });
        save_registry(&path, &registry).unwrap();
        assert_eq!(load_registry(&path), registry);

        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_registry(&path), PluginRegistry::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ofx_scan_finds_bundles_and_tolerates_missing_root() {
        let root = temp_dir("ofx");
        let flare = root.join("LensFlare.ofx");
        std::fs::create_dir_all(flare.join("Contents")).unwrap();
        std::fs::write(
            flare.join("Contents").join("Info.plist"),
            "<plist><dict><key>CFBundleIdentifier</key><string>com.example.lensflare</string>\
             <key>CFBundleName</key><string>Lens Flare</string></dict></plist>",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("nested").join("Bare.ofx")).unwrap();
        std::fs::create_dir_all(root.join("not-a-bundle")).unwrap();

        let found = find_ofx_bundles(&root);
        assert_eq!(found.len(), 2);
        let flare = found.iter().find(|b| b.name == "Lens Flare").unwrap();
        assert_eq!(flare.identifier, "com.example.lensflare");
        let bare = found.iter().find(|b| b.name == "Bare").unwrap();
        assert_eq!(bare.identifier, "local.bare");
        assert!(find_ofx_bundles(&root.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
