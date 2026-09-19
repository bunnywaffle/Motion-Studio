pub mod asset;
pub mod blend_mode;
pub mod color;
pub mod composition;
pub mod error;
pub mod keyframe;
pub mod layer;
pub mod marker;
pub mod matte;
pub mod project;
pub mod property;
pub mod timecode;
pub mod transform;
pub mod vec2;

// Re-export primary types at crate root for ergonomic use
pub use asset::{Asset, AssetType};
pub use blend_mode::BlendMode;
pub use color::Color;
pub use composition::Composition;
pub use error::{ColorError, ProjectError, TimeCodeError, ValidationError};
pub use keyframe::{
    evaluate_cubic_bezier, evaluate_keyframe_track, interpolate_keyframes, Extrapolation,
    Interpolate, Keyframe, KeyframeInterpolation, KeyframeTangent,
};
pub use layer::{Layer, LayerSource, ShapeType};
pub use marker::Marker;
pub use matte::TrackMatteMode;
pub use project::{Project, ProjectSettings, CURRENT_FORMAT_VERSION};
pub use property::Property;
pub use timecode::TimeCode;
pub use transform::Transform;
pub use vec2::Vec2;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_project_construction_and_defaults() {
        let project = Project::default();
        assert_eq!(project.format_version, CURRENT_FORMAT_VERSION);
        assert_eq!(project.name, "Untitled Project");
        assert!(project.compositions.is_empty());
        assert!(project.assets.is_empty());
        assert_eq!(project.settings.working_color_space, "sRGB");
        assert_eq!(project.settings.audio_sample_rate, 48_000);
        assert_eq!(project.settings.start_timecode.frames(), 0);

        let custom = Project::new("proj_custom", "My Awesome Animation");
        assert_eq!(custom.id, "proj_custom");
        assert_eq!(custom.name, "My Awesome Animation");
        assert_eq!(custom.format_version, 1);
    }

    #[test]
    fn test_composition_creation_and_aspect_ratio() {
        let comp = Composition::hd_1080p_30fps("comp_main", "Main Composition", 10.0);
        assert_eq!(comp.id, "comp_main");
        assert_eq!(comp.name, "Main Composition");
        assert_eq!(comp.width, 1920);
        assert_eq!(comp.height, 1080);
        assert!((comp.frame_rate - 30.0).abs() < 1e-6);
        assert_eq!(comp.duration_frames(), 300);
        assert!((comp.duration_seconds() - 10.0).abs() < 1e-6);
        assert!((comp.aspect_ratio() - (16.0 / 9.0)).abs() < 1e-6);
        assert_eq!(comp.background_color, Color::BLACK);

        // Validation of valid composition passes
        assert!(comp.validate().is_ok());
    }

    #[test]
    fn test_composition_layer_ordering_and_movement() {
        let mut comp = Composition::hd_1080p_30fps("comp_order", "Order Test", 5.0);

        let layer1 = Layer::solid(
            "layer_1",
            "Background Solid",
            Color::BLACK,
            1920,
            1080,
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(150, 30.0),
        );
        let layer2 = Layer::text(
            "layer_2",
            "Title Text",
            "Hello World",
            "Inter",
            48.0,
            Color::WHITE,
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(150, 30.0),
        );
        let layer3 = Layer::shape(
            "layer_3",
            "Accent Box",
            ShapeType::Rectangle {
                width: Property::new("Width", 200.0),
                height: Property::new("Height", 100.0),
                corner_radius: Property::new("Corner Radius", 8.0),
            },
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(150, 30.0),
        );

        comp.add_layer(layer1).unwrap();
        comp.add_layer(layer2).unwrap();
        comp.add_layer(layer3).unwrap();

        assert_eq!(comp.layers.len(), 3);
        assert_eq!(comp.layers[0].id, "layer_1");
        assert_eq!(comp.layers[1].id, "layer_2");
        assert_eq!(comp.layers[2].id, "layer_3");

        // Move layer 2 to index 0
        comp.move_layer(2, 0).unwrap();
        assert_eq!(comp.layers[0].id, "layer_3");
        assert_eq!(comp.layers[1].id, "layer_1");
        assert_eq!(comp.layers[2].id, "layer_2");

        // Reorder layer by ID
        comp.reorder_layer("layer_1", 2).unwrap();
        assert_eq!(comp.layers[0].id, "layer_3");
        assert_eq!(comp.layers[1].id, "layer_2");
        assert_eq!(comp.layers[2].id, "layer_1");

        // Out of bounds movement
        assert!(comp.move_layer(10, 0).is_err());
        assert!(comp.move_layer(0, 10).is_err());

        // Remove layer
        let removed = comp.remove_layer("layer_2");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, "layer_2");
        assert_eq!(comp.layers.len(), 2);
        assert_eq!(comp.layers[0].id, "layer_3");
        assert_eq!(comp.layers[1].id, "layer_1");
    }

    #[test]
    fn test_composition_timing_validation() {
        // Invalid dimensions
        let comp_bad_dims = Composition::new(
            "comp_0",
            "Bad Dims",
            0,
            1080,
            30.0,
            TimeCode::from_frames(30, 30.0),
        );
        assert!(matches!(
            comp_bad_dims.validate(),
            Err(ValidationError::InvalidDimensions { .. })
        ));

        // Invalid frame rate
        let comp_bad_fps = Composition::new(
            "comp_0",
            "Bad FPS",
            1920,
            1080,
            0.0,
            TimeCode::from_frames(30, 30.0),
        );
        assert!(matches!(
            comp_bad_fps.validate(),
            Err(ValidationError::InvalidFrameRate(..))
        ));

        // Invalid duration
        let comp_bad_dur = Composition::new(
            "comp_0",
            "Bad Dur",
            1920,
            1080,
            30.0,
            TimeCode::from_frames(0, 30.0),
        );
        assert!(matches!(
            comp_bad_dur.validate(),
            Err(ValidationError::InvalidDuration(..))
        ));

        // Invalid layer timing: in_point > out_point
        let mut comp = Composition::hd_1080p_30fps("comp_valid", "Timing Validation", 5.0);
        let mut bad_layer = Layer::solid(
            "layer_bad_timing",
            "Bad Timing",
            Color::BLACK,
            1920,
            1080,
            TimeCode::from_frames(100, 30.0),
            TimeCode::from_frames(50, 30.0),
        );
        // set_timing error check
        assert!(bad_layer
            .set_timing(
                TimeCode::from_frames(100, 30.0),
                TimeCode::from_frames(50, 30.0)
            )
            .is_err());

        comp.add_layer(bad_layer).unwrap();
        assert!(matches!(
            comp.validate(),
            Err(ValidationError::InvalidLayerTiming { .. })
        ));
    }

    #[test]
    fn test_parenting_and_cycle_detection() {
        let mut comp = Composition::hd_1080p_30fps("comp_parent", "Parenting Test", 10.0);

        let l1 = Layer::solid(
            "l1",
            "Layer 1",
            Color::RED,
            100,
            100,
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(100, 30.0),
        );
        let mut l2 = Layer::solid(
            "l2",
            "Layer 2",
            Color::GREEN,
            100,
            100,
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(100, 30.0),
        );
        let mut l3 = Layer::solid(
            "l3",
            "Layer 3",
            Color::BLUE,
            100,
            100,
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(100, 30.0),
        );

        // Valid chain: l3 -> l2 -> l1
        l3.set_parent(Some("l2"));
        l2.set_parent(Some("l1"));
        comp.add_layer(l1).unwrap();
        comp.add_layer(l2).unwrap();
        comp.add_layer(l3).unwrap();
        assert!(comp.validate_parenting().is_ok());

        // Self parenting
        comp.get_layer_mut("l1").unwrap().set_parent(Some("l1"));
        assert!(matches!(
            comp.validate_parenting(),
            Err(ValidationError::SelfParenting(..))
        ));

        // Cycle: l1 -> l3 (closing l1 -> l3 -> l2 -> l1)
        comp.get_layer_mut("l1").unwrap().set_parent(Some("l3"));
        assert!(matches!(
            comp.validate_parenting(),
            Err(ValidationError::ParentCycleDetected { .. })
        ));

        // Non-existent parent
        comp.get_layer_mut("l1")
            .unwrap()
            .set_parent(Some("non_existent"));
        assert!(matches!(
            comp.validate_parenting(),
            Err(ValidationError::ParentNotFound { .. })
        ));
    }

    #[test]
    fn test_property_system_and_defaults() {
        let mut prop = Property::new("Opacity", 100.0f32);
        assert_eq!(prop.name(), "Opacity");
        assert_eq!(*prop.value(), 100.0);
        assert_eq!(*prop, 100.0); // Deref
        assert!(prop.is_default());
        assert!(!prop.is_animated());

        prop.set_value(50.0);
        assert_eq!(*prop, 50.0);
        assert!(!prop.is_default());

        prop.set_animated(true);
        assert!(prop.is_animated());

        prop.reset();
        assert_eq!(*prop, 100.0);
        assert!(prop.is_default());

        // Vec2 Property
        let mut pos_prop = Property::new("Position", Vec2::new(960.0, 540.0));
        assert_eq!(pos_prop.x, 960.0);
        assert_eq!(pos_prop.y, 540.0);
        pos_prop.x = 1000.0;
        assert!(!pos_prop.is_default());
        pos_prop.reset();
        assert_eq!(pos_prop.x, 960.0);
        assert!(pos_prop.is_default());
    }

    #[test]
    fn test_transform_construction_and_defaults() {
        let mut transform = Transform::default();
        assert_eq!(*transform.anchor_point, Vec2::ZERO);
        assert_eq!(*transform.position, Vec2::ZERO);
        assert_eq!(*transform.scale, Vec2::new(100.0, 100.0));
        assert_eq!(*transform.rotation, 0.0);
        assert!(transform.is_default());

        transform.position.set_value(Vec2::new(1920.0 / 2.0, 1080.0 / 2.0));
        transform.rotation.set_value(45.0);
        assert!(!transform.is_default());

        transform.reset_all();
        assert!(transform.is_default());
        assert_eq!(*transform.rotation, 0.0);
    }

    #[test]
    fn test_asset_tracking_and_references() {
        let mut project = Project::with_defaults("Asset Test Project");

        let asset1 = Asset::from_path("asset_logo", "Logo", PathBuf::from("assets/logo.png"));
        assert!(asset1.is_image());
        assert_eq!(asset1.asset_type, AssetType::Image);

        let asset2 = Asset::from_path("asset_bg", "Background Video", PathBuf::from("media/clip.mp4"));
        assert!(asset2.is_video());
        assert_eq!(asset2.asset_type, AssetType::Video);

        project.add_asset(asset1).unwrap();
        project.add_asset(asset2).unwrap();

        // Adding duplicate asset ID errors
        let duplicate = Asset::new("asset_logo", "Dup", PathBuf::from("dup.png"), AssetType::Image);
        assert!(matches!(
            project.add_asset(duplicate),
            Err(ValidationError::DuplicateAssetId(..))
        ));

        // Create composition with layers referencing assets
        let mut comp = Composition::hd_1080p_30fps("comp_main", "Comp Main", 10.0);
        let image_layer = Layer::image(
            "layer_img",
            "Logo Layer",
            "asset_logo",
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(300, 30.0),
        );
        let video_layer = Layer::video(
            "layer_vid",
            "Video Layer",
            "asset_bg",
            TimeCode::zero(30.0),
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(300, 30.0),
        );

        comp.add_layer(image_layer).unwrap();
        comp.add_layer(video_layer).unwrap();
        project.add_composition(comp).unwrap();

        // Find layers referencing asset_logo
        let refs = project.find_layers_referencing_asset("asset_logo");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].1.id, "layer_img");

        // Find layers referencing asset_bg
        let refs_vid = project.find_layers_referencing_asset("asset_bg");
        assert_eq!(refs_vid.len(), 1);
        assert_eq!(refs_vid[0].1.id, "layer_vid");

        // Validate passes
        assert!(project.validate().is_ok());

        // Remove asset that is referenced -> project validation fails
        let removed = project.remove_asset("asset_logo");
        assert!(removed.is_some());
        assert!(matches!(
            project.validate(),
            Err(ValidationError::AssetNotFound(ref id)) if id == "asset_logo"
        ));
    }

    #[test]
    fn test_nested_composition_references_and_cycle_detection() {
        let mut project = Project::with_defaults("Nested Comp Project");

        let mut comp_a = Composition::hd_1080p_30fps("comp_a", "Comp A", 5.0);
        let comp_b = Composition::hd_1080p_30fps("comp_b", "Comp B", 5.0);

        // Comp A nests Comp B
        comp_a
            .add_layer(Layer::nested_composition(
                "nested_layer_b",
                "Nested Comp B",
                "comp_b",
                TimeCode::from_frames(0, 30.0),
                TimeCode::from_frames(150, 30.0),
            ))
            .unwrap();

        project.add_composition(comp_a).unwrap();
        project.add_composition(comp_b).unwrap();

        assert!(project.validate().is_ok());

        // Introduce nested composition cycle: Comp B nests Comp A
        project
            .get_composition_mut("comp_b")
            .unwrap()
            .add_layer(Layer::nested_composition(
                "nested_layer_a",
                "Nested Comp A",
                "comp_a",
                TimeCode::from_frames(0, 30.0),
                TimeCode::from_frames(150, 30.0),
            ))
            .unwrap();

        assert!(matches!(
            project.validate(),
            Err(ValidationError::CircularNestedComposition { .. })
        ));
    }

    #[test]
    fn test_color_operations_and_parsing() {
        // Hex 6-char
        let c1 = Color::from_hex("#FF8000").unwrap();
        assert_eq!(c1.to_rgba_u8(), (255, 128, 0, 255));
        assert_eq!(c1.to_hex_rgb(), "#FF8000");

        // Hex 3-char
        let c2 = Color::from_hex("#F80").unwrap();
        assert_eq!(c2.to_rgba_u8(), (255, 136, 0, 255));

        // Hex 8-char
        let c3 = Color::from_hex("#00FF0080").unwrap();
        assert_eq!(c3.to_rgba_u8(), (0, 255, 0, 128));

        // Invalid hex
        assert!(Color::from_hex("#ZZZZZZ").is_err());
        assert!(Color::from_hex("12").is_err());

        // Clamping
        let clamped = Color::rgba(-0.5, 1.5, 0.5, 2.0);
        assert_eq!(clamped.r, 0.0);
        assert_eq!(clamped.g, 1.0);
        assert_eq!(clamped.b, 0.5);
        assert_eq!(clamped.a, 1.0);
    }

    #[test]
    fn test_timecode_conversions_and_arithmetic() {
        let tc = TimeCode::from_timecode_str("01:02:03:15", 30.0).unwrap();
        assert_eq!(tc.to_timecode_str(), "01:02:03:15");

        let expected_frames = ((60 + 2) * 60 + 3) * 30 + 15;
        assert_eq!(tc.frames(), expected_frames);

        // Arithmetic
        let tc1 = TimeCode::from_frames(45, 30.0);
        let tc2 = TimeCode::from_frames(15, 30.0);
        let sum = tc1 + tc2;
        assert_eq!(sum.frames(), 60);
        let diff = tc1 - tc2;
        assert_eq!(diff.frames(), 30);

        // Comparison
        assert!(tc1 > tc2);
        assert!(tc2 < tc1);
        assert_eq!(tc1, TimeCode::from_seconds(1.5, 30.0));
    }

    #[test]
    fn test_layer_sources_all_variants() {
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc100 = TimeCode::from_frames(100, 30.0);

        let solid = Layer::solid("s", "Solid", Color::RED, 1920, 1080, tc0, tc100);
        assert!(matches!(solid.source, LayerSource::Solid { .. }));

        let img = Layer::image("i", "Image", "asset_1", tc0, tc100);
        assert!(matches!(img.source, LayerSource::Image { .. }));

        let vid = Layer::video("v", "Video", "asset_2", tc0, tc0, tc100);
        assert!(matches!(vid.source, LayerSource::Video { .. }));

        let txt = Layer::text("t", "Text", "Sample", "Arial", 32.0, Color::WHITE, tc0, tc100);
        assert!(matches!(txt.source, LayerSource::Text { .. }));

        let shape = Layer::shape(
            "sh",
            "Shape",
            ShapeType::Ellipse {
                radius_x: Property::new("Radius X", 50.0),
                radius_y: Property::new("Radius Y", 50.0),
            },
            tc0,
            tc100,
        );
        assert!(matches!(shape.source, LayerSource::Shape { .. }));

        let nested = Layer::nested_composition("nc", "PreComp", "comp_sub", tc0, tc100);
        assert!(matches!(nested.source, LayerSource::NestedComposition { .. }));

        let proc = Layer::procedural("p", "Fractal Noise", "fractal_noise", tc0, tc100);
        assert!(matches!(proc.source, LayerSource::Procedural { .. }));
    }

    #[test]
    fn test_markers_on_compositions_and_layers() {
        let mut comp = Composition::hd_1080p_30fps("comp_m", "Marker Comp", 10.0);
        let m1 = Marker::new("m1", TimeCode::from_frames(30, 30.0), "Beat drop")
            .with_color(Color::RED);
        let m2 = Marker::new("m2", TimeCode::from_frames(90, 30.0), "Transition")
            .with_duration(TimeCode::from_frames(15, 30.0));

        comp.add_marker(m1);
        comp.add_marker(m2);

        assert_eq!(comp.markers.len(), 2);
        assert_eq!(comp.markers[0].comment, "Beat drop");
        assert_eq!(comp.markers[0].color, Some(Color::RED));
        assert_eq!(comp.markers[1].duration.unwrap().frames(), 15);

        let removed = comp.remove_marker("m1");
        assert!(removed.is_some());
        assert_eq!(comp.markers.len(), 1);
        assert_eq!(comp.markers[0].id, "m2");
    }

    #[test]
    fn test_blend_modes() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);
        assert_eq!(BlendMode::Normal.as_str(), "Normal");
        assert_eq!(BlendMode::Multiply.as_str(), "Multiply");
        assert_eq!(BlendMode::Screen.as_str(), "Screen");
        assert_eq!(BlendMode::Overlay.as_str(), "Overlay");

        assert_eq!(BlendMode::ALL.len(), 19);
        assert_eq!(BlendMode::ALL[0], BlendMode::Normal);

        assert_eq!(BlendMode::from_name("multiply"), Some(BlendMode::Multiply));
        assert_eq!(BlendMode::from_name("SCREEN"), Some(BlendMode::Screen));
        assert_eq!(BlendMode::from_name("Color Dodge"), Some(BlendMode::ColorDodge));
        assert_eq!(BlendMode::from_name("invalid_mode"), None);
    }

    #[test]
    fn test_negative_timecode_and_robustness() {
        // Formatting negative timecode
        let neg_tc = TimeCode::from_frames(-30, 30.0);
        assert_eq!(neg_tc.to_timecode_str(), "-00:00:01:00");
        assert_eq!(neg_tc.seconds(), -1.0);

        // Parsing negative timecode
        let parsed_neg = TimeCode::from_timecode_str("-00:00:01:00", 30.0).unwrap();
        assert_eq!(parsed_neg.frames(), -30);
        assert_eq!(parsed_neg, neg_tc);

        // Complex negative timecode: -1 hour, 1 second = -3601 seconds = -108030 frames
        let parsed_complex_neg = TimeCode::from_timecode_str("-01:00:01:00", 30.0).unwrap();
        assert_eq!(parsed_complex_neg.frames(), -108030);
        assert_eq!(parsed_complex_neg.to_timecode_str(), "-01:00:01:00");

        // Reject invalid / NaN frame rate
        assert!(TimeCode::from_timecode_str("00:00:01:00", f64::NAN).is_err());
        assert!(TimeCode::from_timecode_str("00:00:01:00", -10.0).is_err());
        assert!(TimeCode::from_timecode_str("00:00:01:00", 0.0).is_err());

        // Negative component inside body is rejected
        assert!(TimeCode::from_timecode_str("00:-01:00:00", 30.0).is_err());

        // PartialOrd and PartialEq consistency
        let tc_a = TimeCode::from_frames(30, 30.0);
        let tc_b = TimeCode::from_frames(60, 60.0);
        assert_eq!(tc_a, tc_b);
        assert_eq!(tc_a.partial_cmp(&tc_b), Some(std::cmp::Ordering::Equal));
    }

    #[test]
    fn test_color_robustness_and_clamping() {
        // Clamping NaN values
        let nan_color = Color::rgba(f32::NAN, -0.5, 1.5, f32::NAN);
        assert_eq!(nan_color.r, 0.0);
        assert_eq!(nan_color.g, 0.0);
        assert_eq!(nan_color.b, 1.0);
        assert_eq!(nan_color.a, 0.0);

        // Invalid hex characters return InvalidHexDigit
        assert_eq!(
            Color::from_hex("#ZZZZZZ"),
            Err(ColorError::InvalidHexDigit('Z'))
        );
        assert_eq!(
            Color::from_hex("#123G"),
            Err(ColorError::InvalidHexDigit('G'))
        );
    }

    #[test]
    fn test_deep_layer_parenting_and_hierarchy_queries() {
        let mut comp = Composition::hd_1080p_30fps("comp_deep", "Deep Parenting", 10.0);
        const CHAIN_DEPTH: usize = 100;

        // Build a deep parenting chain: layer_0 (root) <- layer_1 <- ... <- layer_99
        let root = Layer::solid(
            "layer_0",
            "Root Layer",
            Color::BLACK,
            1920,
            1080,
            TimeCode::from_frames(0, 30.0),
            TimeCode::from_frames(300, 30.0),
        );
        comp.add_layer(root).unwrap();

        for i in 1..CHAIN_DEPTH {
            let mut layer = Layer::solid(
                format!("layer_{i}"),
                format!("Layer {i}"),
                Color::WHITE,
                1920,
                1080,
                TimeCode::from_frames(0, 30.0),
                TimeCode::from_frames(300, 30.0),
            );
            layer.set_parent(Some(format!("layer_{}", i - 1)));
            comp.add_layer(layer).unwrap();
        }

        // Validate complete 100-layer parenting chain
        assert!(comp.validate_parenting().is_ok());

        // Test root_layers query
        let roots = comp.root_layers();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, "layer_0");

        // Test get_parent query
        assert_eq!(comp.get_parent("layer_99").unwrap().id, "layer_98");
        assert!(comp.get_parent("layer_0").is_none());

        // Test get_children query
        let children_of_0 = comp.get_children("layer_0");
        assert_eq!(children_of_0.len(), 1);
        assert_eq!(children_of_0[0].id, "layer_1");

        // Test get_ancestor_chain query
        let ancestors = comp.get_ancestor_chain("layer_99");
        assert_eq!(ancestors.len(), 99);
        assert_eq!(ancestors[0].id, "layer_98");
        assert_eq!(ancestors[98].id, "layer_0");
    }

    #[test]
    fn test_deep_nested_compositions_graph() {
        let mut project = Project::with_defaults("Deep Nested Comps");
        const DEPTH: usize = 60;

        // Create DEPTH compositions where comp_{i} nests comp_{i-1}
        let comp_0 = Composition::hd_1080p_30fps("comp_0", "Base Comp", 5.0);
        project.add_composition(comp_0).unwrap();

        for i in 1..DEPTH {
            let mut comp = Composition::hd_1080p_30fps(format!("comp_{i}"), format!("Comp {i}"), 5.0);
            comp.add_layer(Layer::nested_composition(
                format!("nested_in_{i}"),
                format!("Nested Ref {i}"),
                format!("comp_{}", i - 1),
                TimeCode::from_frames(0, 30.0),
                TimeCode::from_frames(150, 30.0),
            ))
            .unwrap();
            project.add_composition(comp).unwrap();
        }

        // Graph of 60 nested compositions should validate cleanly
        assert!(project.validate().is_ok());

        // Introduce a cycle at the root: comp_0 nests comp_59
        project
            .get_composition_mut("comp_0")
            .unwrap()
            .add_layer(Layer::nested_composition(
                "cycle_layer",
                "Cycle Trigger",
                format!("comp_{}", DEPTH - 1),
                TimeCode::from_frames(0, 30.0),
                TimeCode::from_frames(150, 30.0),
            ))
            .unwrap();

        assert!(matches!(
            project.validate(),
            Err(ValidationError::CircularNestedComposition { .. })
        ));
    }

    #[test]
    fn test_nan_frame_rate_rejected_in_validate() {
        let comp = Composition::new(
            "comp_nan",
            "NaN FPS",
            1920,
            1080,
            f64::NAN,
            TimeCode::from_frames(100, 30.0),
        );
        assert!(matches!(
            comp.validate(),
            Err(ValidationError::InvalidFrameRate(..))
        ));

        let comp_inf = Composition::new(
            "comp_inf",
            "Inf FPS",
            1920,
            1080,
            f64::INFINITY,
            TimeCode::from_frames(100, 30.0),
        );
        assert!(matches!(
            comp_inf.validate(),
            Err(ValidationError::InvalidFrameRate(..))
        ));
    }

    #[test]
    fn test_property_traits_and_defaults() {
        let mut prop: Property<f32> = Property::default();
        assert_eq!(*prop, 0.0);
        assert_eq!(*prop.default_value(), 0.0);
        assert!(prop.is_default());

        prop.set_value(42.0);
        assert_eq!(*prop, 42.0);
        assert!(!prop.is_default());

        prop.set_default_value(42.0);
        assert!(prop.is_default());

        let named = Property::<f32>::new_default("Custom Prop");
        assert_eq!(named.name(), "Custom Prop");
        assert_eq!(*named, 0.0f32);
        assert_eq!(format!("{named}"), "Custom Prop: 0");
    }

    #[test]
    fn test_layer_source_type_names_and_predicates() {
        let tc0 = TimeCode::zero(30.0);
        let tc100 = TimeCode::from_frames(100, 30.0);

        let solid = Layer::solid("s", "Solid", Color::RED, 100, 100, tc0, tc100);
        assert!(solid.is_solid());
        assert_eq!(solid.source.type_name(), "Solid");
        assert!(solid.is_visible());
        assert!(!solid.is_locked());

        let img = Layer::image("img", "Image", "asset_1", tc0, tc100);
        assert!(img.is_image());
        assert_eq!(img.source.type_name(), "Image");

        let vid = Layer::video("vid", "Video", "asset_2", tc0, tc0, tc100);
        assert!(vid.is_video());
        assert_eq!(vid.source.type_name(), "Video");

        let txt = Layer::text("txt", "Text", "Hello", "Roboto", 24.0, Color::WHITE, tc0, tc100);
        assert!(txt.is_text());
        assert_eq!(txt.source.type_name(), "Text");

        let shape = Layer::shape(
            "shp",
            "Shape",
            ShapeType::Path { path_data: "M 0 0 L 10 10".to_string() },
            tc0,
            tc100,
        );
        assert!(shape.is_shape());
        assert_eq!(shape.source.type_name(), "Shape");

        let nested = Layer::nested_composition("nc", "PreComp", "comp_sub", tc0, tc100);
        assert!(nested.is_nested_composition());
        assert_eq!(nested.source.type_name(), "NestedComposition");

        let proc = Layer::procedural("p", "Noise", "perlin", tc0, tc100);
        assert!(proc.is_procedural());
        assert_eq!(proc.source.type_name(), "Procedural");
    }

    #[test]
    fn test_asset_font_vector_and_project_layer_search() {
        let font_asset = Asset::from_path("font_1", "Inter Font", PathBuf::from("fonts/inter.woff2"));
        assert!(font_asset.is_font());

        let svg_asset = Asset::from_path("svg_1", "Icon", PathBuf::from("icons/logo.svg"));
        assert!(svg_asset.is_vector());

        let mut project = Project::with_defaults("Search Test Project");
        let mut comp1 = Composition::hd_1080p_30fps("c1", "Comp 1", 5.0);
        let mut comp2 = Composition::hd_1080p_30fps("c2", "Comp 2", 5.0);

        let l1 = Layer::solid("l1", "L1", Color::RED, 10, 10, TimeCode::zero(30.0), TimeCode::from_frames(10, 30.0));
        let l2 = Layer::solid("l2", "L2", Color::BLUE, 10, 10, TimeCode::zero(30.0), TimeCode::from_frames(10, 30.0));

        comp1.add_layer(l1).unwrap();
        comp2.add_layer(l2).unwrap();

        project.add_composition(comp1).unwrap();
        project.add_composition(comp2).unwrap();

        assert_eq!(project.total_layers_count(), 2);

        let found_l2 = project.find_layer("l2");
        assert!(found_l2.is_some());
        assert_eq!(found_l2.unwrap().0.id, "c2");
        assert_eq!(found_l2.unwrap().1.id, "l2");

        let not_found = project.find_layer("l_missing");
        assert!(not_found.is_none());

        let found_l1_mut = project.find_layer_mut("l1");
        assert!(found_l1_mut.is_some());
        found_l1_mut.unwrap().name = "Renamed L1".to_string();
        assert_eq!(project.find_layer("l1").unwrap().1.name, "Renamed L1");
    }

    #[test]
    fn test_section_16_project_json_format() {
        let mut project = Project::new("proj_motion_01", "Neon Intro Sequence");
        let mut comp = Composition::hd_1080p_30fps("comp_main", "Main Composition", 10.0);
        let layer = Layer::solid(
            "layer_bg",
            "Dark Solid",
            Color::rgba(0.05, 0.05, 0.08, 1.0),
            1920,
            1080,
            TimeCode::zero(30.0),
            TimeCode::from_frames(300, 30.0),
        );
        comp.add_layer(layer).unwrap();
        project.add_composition(comp).unwrap();

        let asset = Asset::from_path("asset_audio", "Beat Track", PathBuf::from("audio/beat.wav"));
        project.add_asset(asset).unwrap();

        // Serialize to JSON
        let json_str = project.to_json_pretty().expect("Failed to serialize project");

        // Parse as generic JSON Value to verify Section 16 schema keys
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("Valid JSON");
        assert_eq!(v["format_version"], 1);
        assert!(v["project"].is_object(), "Top-level 'project' object missing");
        assert_eq!(v["project"]["id"], "proj_motion_01");
        assert_eq!(v["project"]["name"], "Neon Intro Sequence");
        assert!(v["compositions"].is_array(), "'compositions' array missing");
        assert_eq!(v["compositions"].as_array().unwrap().len(), 1);
        assert!(v["assets"].is_array(), "'assets' array missing");
        assert_eq!(v["assets"].as_array().unwrap().len(), 1);
        assert!(v["settings"].is_object(), "'settings' object missing");
        assert_eq!(v["settings"]["working_color_space"], "sRGB");
        assert_eq!(v["settings"]["audio_sample_rate"], 48000);

        // Verify layer serialization structure
        let comp_val = &v["compositions"][0];
        assert_eq!(comp_val["width"], 1920);
        assert_eq!(comp_val["height"], 1080);
        let layer_val = &comp_val["layers"][0];
        assert_eq!(layer_val["id"], "layer_bg");
        assert_eq!(layer_val["source"]["type"], "solid");
        assert_eq!(layer_val["source"]["width"], 1920);
        assert_eq!(layer_val["blend_mode"], "normal");
    }

    #[test]
    fn test_project_serialization_roundtrip_all_layer_types() {
        let mut project = Project::new("proj_comprehensive", "All Layer Types Test");

        // Add assets
        let img_asset = Asset::from_path("a_img", "Background PNG", PathBuf::from("assets/bg.png"));
        let vid_asset = Asset::from_path("a_vid", "Hero Clip", PathBuf::from("assets/hero.mp4"));
        project.add_asset(img_asset).unwrap();
        project.add_asset(vid_asset).unwrap();

        // Create sub-composition
        let sub_comp = Composition::hd_1080p_30fps("comp_sub", "Sub Comp", 3.0);
        project.add_composition(sub_comp).unwrap();

        // Create main composition
        let mut main_comp = Composition::hd_1080p_30fps("comp_main", "Main Comp", 10.0);
        main_comp.add_marker(
            Marker::new("m_intro", TimeCode::from_frames(30, 30.0), "Intro Marker")
                .with_duration(TimeCode::from_frames(10, 30.0))
                .with_color(Color::GREEN),
        );

        let tc0 = TimeCode::zero(30.0);
        let tc300 = TimeCode::from_frames(300, 30.0);

        // Layer 1: Solid
        let l_solid = Layer::solid("l_solid", "Solid", Color::RED, 1920, 1080, tc0, tc300);

        // Layer 2: Image
        let l_img = Layer::image("l_img", "Image Layer", "a_img", tc0, tc300);

        // Layer 3: Video
        let l_vid = Layer::video("l_vid", "Video Layer", "a_vid", tc0, tc0, tc300);

        // Layer 4: Text
        let mut l_text = Layer::text("l_text", "Text Layer", "Dynamic Title", "Helvetica", 72.0, Color::WHITE, tc0, tc300);
        l_text.blend_mode = BlendMode::Overlay;

        // Layer 5: Shape (Rectangle)
        let l_shape_rect = Layer::shape(
            "l_rect",
            "Rect Layer",
            ShapeType::Rectangle {
                width: Property::new("Width", 400.0),
                height: Property::new("Height", 200.0),
                corner_radius: Property::new("Corner Radius", 16.0),
            },
            tc0,
            tc300,
        );

        // Layer 6: Shape (Ellipse)
        let l_shape_ellipse = Layer::shape(
            "l_ellipse",
            "Ellipse Layer",
            ShapeType::Ellipse {
                radius_x: Property::new("Radius X", 100.0),
                radius_y: Property::new("Radius Y", 150.0),
            },
            tc0,
            tc300,
        );

        // Layer 7: Shape (Path)
        let l_shape_path = Layer::shape(
            "l_path",
            "Path Layer",
            ShapeType::Path {
                path_data: "M 0 0 C 10 20, 30 40, 50 50 Z".to_string(),
            },
            tc0,
            tc300,
        );

        // Layer 8: Nested Composition
        let mut l_nested = Layer::nested_composition("l_nested", "PreComp Layer", "comp_sub", tc0, tc300);
        l_nested.set_parent(Some("l_solid"));

        // Layer 9: Procedural
        let mut l_proc = Layer::procedural("l_proc", "Noise Generator", "simplex_noise", tc0, tc300);
        l_proc.opacity.set_value(75.0);
        l_proc.blend_mode = BlendMode::Screen;

        main_comp.add_layer(l_solid).unwrap();
        main_comp.add_layer(l_img).unwrap();
        main_comp.add_layer(l_vid).unwrap();
        main_comp.add_layer(l_text).unwrap();
        main_comp.add_layer(l_shape_rect).unwrap();
        main_comp.add_layer(l_shape_ellipse).unwrap();
        main_comp.add_layer(l_shape_path).unwrap();
        main_comp.add_layer(l_nested).unwrap();
        main_comp.add_layer(l_proc).unwrap();

        project.add_composition(main_comp).unwrap();

        // Roundtrip serialization
        let json = project.to_json_pretty().expect("Serialize to JSON");
        let deserialized = Project::from_json(&json).expect("Deserialize from JSON");

        // Verify equality
        assert_eq!(project, deserialized);
        assert!(deserialized.validate().is_ok());

        // Verify specific values
        let restored_main = deserialized.get_composition("comp_main").unwrap();
        assert_eq!(restored_main.layers.len(), 9);
        assert_eq!(restored_main.layers[7].parent_id.as_deref(), Some("l_solid"));
        assert_eq!(restored_main.layers[3].blend_mode, BlendMode::Overlay);
        assert_eq!(restored_main.layers[8].blend_mode, BlendMode::Screen);
        assert_eq!(*restored_main.layers[8].opacity, 75.0);
        assert_eq!(restored_main.markers.len(), 1);
        assert_eq!(restored_main.markers[0].comment, "Intro Marker");
    }

    #[test]
    fn test_project_deserialization_from_flat_format() {
        // Test backwards/alternate compatibility where 'id' and 'name' are at top level
        let flat_json = r#"{
            "format_version": 1,
            "id": "proj_flat_01",
            "name": "Flat Style Project",
            "compositions": [],
            "assets": [],
            "settings": {
                "working_color_space": "Rec709",
                "audio_sample_rate": 44100,
                "start_timecode": {
                    "frames": 30,
                    "frame_rate": 30.0
                }
            }
        }"#;

        let proj = Project::from_json(flat_json).expect("Deserialize flat JSON");
        assert_eq!(proj.id, "proj_flat_01");
        assert_eq!(proj.name, "Flat Style Project");
        assert_eq!(proj.settings.working_color_space, "Rec709");
        assert_eq!(proj.settings.audio_sample_rate, 44100);
        assert_eq!(proj.settings.start_timecode.frames(), 30);
    }

    #[test]
    fn test_blend_mode_serde_all_variants_and_flexibility() {
        for mode in BlendMode::ALL {
            let serialized = serde_json::to_string(&mode).unwrap();
            let deserialized: BlendMode = serde_json::from_str(&serialized).unwrap();
            assert_eq!(mode, deserialized);

            // Verify snake_case representation
            let raw_str = serialized.trim_matches('"');
            assert_eq!(raw_str, mode.as_snake_case());
        }

        // Test flexible parsing during deserialization
        let m1: BlendMode = serde_json::from_str(r#""color_dodge""#).unwrap();
        assert_eq!(m1, BlendMode::ColorDodge);

        let m2: BlendMode = serde_json::from_str(r#""Color Dodge""#).unwrap();
        assert_eq!(m2, BlendMode::ColorDodge);

        let m3: BlendMode = serde_json::from_str(r#""colordodge""#).unwrap();
        assert_eq!(m3, BlendMode::ColorDodge);

        let m4: BlendMode = serde_json::from_str(r#""HARD-LIGHT""#).unwrap();
        assert_eq!(m4, BlendMode::HardLight);

        // Invalid variant
        assert!(serde_json::from_str::<BlendMode>(r#""non_existent_mode""#).is_err());
    }

    #[test]
    fn test_project_file_save_and_load_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("test_project_{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));

        let mut project = Project::new("proj_io_test", "File IO Verification");
        let comp = Composition::hd_1080p_30fps("comp_file", "File Comp", 2.0);
        project.add_composition(comp).unwrap();

        // Save to file
        project.save_to_file(&temp_file).expect("Save to file succeeds");

        // Load from file
        let loaded = Project::load_from_file(&temp_file).expect("Load from file succeeds");
        assert_eq!(project, loaded);

        // Cleanup
        let _ = std::fs::remove_file(temp_file);
    }

    #[test]
    fn test_deserialization_error_reporting() {
        let bad_json = r#"{
            "format_version": "not_a_number"
        }"#;

        let res = Project::from_json(bad_json);
        assert!(res.is_err());
    }

    #[test]
    fn test_track_matte_and_solo_serialization_roundtrip() {
        let mut project = Project::new("proj_matte_test", "Matte and Solo Test");
        let mut comp = Composition::hd_1080p_30fps("comp_m", "Matte Comp", 5.0);
        let tc0 = TimeCode::zero(30.0);
        let tc100 = TimeCode::from_frames(100, 30.0);

        let l_matte = Layer::solid("l_src", "Matte Source", Color::WHITE, 200, 200, tc0, tc100)
            .with_solo(true);

        let l_masked = Layer::solid("l_target", "Masked Layer", Color::RED, 1920, 1080, tc0, tc100)
            .with_matte(TrackMatteMode::AlphaInverted, Some("l_src"));

        comp.add_layer(l_matte).unwrap();
        comp.add_layer(l_masked).unwrap();
        project.add_composition(comp).unwrap();

        // Roundtrip
        let json = project.to_json_pretty().expect("Serialize");
        let restored = Project::from_json(&json).expect("Deserialize");
        assert_eq!(project, restored);

        let r_comp = restored.get_composition("comp_m").unwrap();
        assert!(r_comp.layers[0].is_solo());
        assert_eq!(r_comp.layers[1].matte_mode, TrackMatteMode::AlphaInverted);
        assert_eq!(r_comp.layers[1].matte_layer_id.as_deref(), Some("l_src"));
        assert!(r_comp.layers[1].has_matte());
    }

    #[test]
    fn test_keyframe_hold_and_linear_interpolation() {
        let fps = 30.0;
        let t0 = TimeCode::from_frames(0, fps);
        let t30 = TimeCode::from_frames(30, fps); // 1.0s
        let t60 = TimeCode::from_frames(60, fps); // 2.0s

        // Hold keyframes: [0s: 10.0, Hold] -> [1s: 20.0, Hold] -> [2s: 30.0]
        let k0 = Keyframe::hold(t0, 10.0f32);
        let k1 = Keyframe::hold(t30, 20.0f32);
        let k2 = Keyframe::linear(t60, 30.0f32);

        let kfs = vec![k0, k1, k2];

        // Before 1.0s, value must hold at 10.0
        assert_eq!(evaluate_keyframe_track(&kfs, 0.0, &0.0, Extrapolation::Hold, Extrapolation::Hold), 10.0);
        assert_eq!(evaluate_keyframe_track(&kfs, 0.5, &0.0, Extrapolation::Hold, Extrapolation::Hold), 10.0);
        assert_eq!(evaluate_keyframe_track(&kfs, 0.999, &0.0, Extrapolation::Hold, Extrapolation::Hold), 10.0);

        // Exactly at 1.0s, value steps to 20.0
        assert_eq!(evaluate_keyframe_track(&kfs, 1.0, &0.0, Extrapolation::Hold, Extrapolation::Hold), 20.0);
        assert_eq!(evaluate_keyframe_track(&kfs, 1.5, &0.0, Extrapolation::Hold, Extrapolation::Hold), 20.0);

        // Exactly at 2.0s, value steps to 30.0
        assert_eq!(evaluate_keyframe_track(&kfs, 2.0, &0.0, Extrapolation::Hold, Extrapolation::Hold), 30.0);

        // Linear keyframes: [0s: 0.0] -> [1s: 100.0]
        let lin0 = Keyframe::linear(t0, 0.0f32);
        let lin1 = Keyframe::linear(t30, 100.0f32);
        let lin_kfs = vec![lin0, lin1];

        // Exact intermediate points and arbitrary floating-point times
        let v_mid = evaluate_keyframe_track(&lin_kfs, 0.5, &0.0, Extrapolation::Hold, Extrapolation::Hold);
        assert!((v_mid - 50.0).abs() < 1e-5);

        let v_quarter = evaluate_keyframe_track(&lin_kfs, 0.25, &0.0, Extrapolation::Hold, Extrapolation::Hold);
        assert!((v_quarter - 25.0).abs() < 1e-5);

        let v_arb = evaluate_keyframe_track(&lin_kfs, 0.732, &0.0, Extrapolation::Hold, Extrapolation::Hold);
        assert!((v_arb - 73.2).abs() < 1e-4);
    }

    #[test]
    fn test_cubic_bezier_curve_evaluation() {
        // 1. Diagonal linear control points (1/3, 1/3) and (2/3, 2/3) must yield pure linear result
        let p_lin_out = KeyframeTangent::linear_out();
        let p_lin_in = KeyframeTangent::linear_in();
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let s = evaluate_cubic_bezier(t, p_lin_out, p_lin_in);
            assert!((s - t).abs() < 1e-4, "Failed for t = {t}, got {s}");
        }

        // 2. Ease In Out curve (0.42, 0.0) and (0.58, 1.0)
        let ease_out = KeyframeTangent::ease_in_out_out();
        let ease_in = KeyframeTangent::ease_in_out_in();

        // Boundaries
        assert_eq!(evaluate_cubic_bezier(0.0, ease_out, ease_in), 0.0);
        assert_eq!(evaluate_cubic_bezier(1.0, ease_out, ease_in), 1.0);

        // Symmetry around midpoint: at t = 0.5, ease_in_out should be 0.5
        let mid = evaluate_cubic_bezier(0.5, ease_out, ease_in);
        assert!((mid - 0.5).abs() < 1e-3, "Ease in out mid was {mid}");

        // Slow start (t = 0.2 must be significantly less than 0.2)
        let slow_start = evaluate_cubic_bezier(0.2, ease_out, ease_in);
        assert!(slow_start < 0.15, "Expected slow start, got {slow_start}");

        // Slow finish (t = 0.8 must be significantly greater than 0.8)
        let slow_finish = evaluate_cubic_bezier(0.8, ease_out, ease_in);
        assert!(slow_finish > 0.85, "Expected slow finish, got {slow_finish}");

        // Interpolate keyframes with Bezier
        let fps = 30.0;
        let k0 = Keyframe::bezier(
            TimeCode::from_frames(0, fps),
            0.0f32,
            None,
            Some(KeyframeTangent::ease_in_out_out()),
        );
        let k1 = Keyframe::bezier(
            TimeCode::from_frames(30, fps),
            100.0f32,
            Some(KeyframeTangent::ease_in_out_in()),
            None,
        );

        let v_eased_mid = interpolate_keyframes(&k0, &k1, 0.5);
        assert!((v_eased_mid - 50.0).abs() < 0.1);

        let v_eased_early = interpolate_keyframes(&k0, &k1, 0.2);
        assert!(v_eased_early < 15.0);
    }

    #[test]
    fn test_keyframe_boundary_extrapolation() {
        let fps = 30.0;
        // Two keyframes: [1.0s: 100.0] -> [3.0s: 300.0] (duration = 2.0s, slope = 100.0 units/sec)
        let k0 = Keyframe::linear(TimeCode::from_seconds(1.0, fps), 100.0f32);
        let k1 = Keyframe::linear(TimeCode::from_seconds(3.0, fps), 300.0f32);
        let kfs = vec![k0, k1];

        // 1. Hold Extrapolation
        let pre_hold = evaluate_keyframe_track(&kfs, 0.0, &0.0, Extrapolation::Hold, Extrapolation::Hold);
        assert_eq!(pre_hold, 100.0);
        let post_hold = evaluate_keyframe_track(&kfs, 5.0, &0.0, Extrapolation::Hold, Extrapolation::Hold);
        assert_eq!(post_hold, 300.0);

        // 2. Linear Extrapolation
        // At t = 0.0s (1.0s before k0), value should project to 100 - (1.0 * 100) = 0.0
        let pre_lin = evaluate_keyframe_track(&kfs, 0.0, &0.0, Extrapolation::Linear, Extrapolation::Linear);
        assert!((pre_lin - 0.0).abs() < 1e-4);

        // At t = 4.0s (1.0s after k1), value should project to 300 + (1.0 * 100) = 400.0
        let post_lin = evaluate_keyframe_track(&kfs, 4.0, &0.0, Extrapolation::Linear, Extrapolation::Linear);
        assert!((post_lin - 400.0).abs() < 1e-4);

        // 3. Cycle Extrapolation (Duration 2.0s, span [1.0, 3.0])
        // t = 3.5s -> wraps to 1.5s -> value 150.0
        let cycle_val = evaluate_keyframe_track(&kfs, 3.5, &0.0, Extrapolation::Cycle, Extrapolation::Cycle);
        assert!((cycle_val - 150.0).abs() < 1e-3);

        // 4. PingPong Extrapolation
        // t = 4.0s -> 1.0s past end in first reflection -> moves from 300 back towards 100 -> value 200.0
        let pingpong_val = evaluate_keyframe_track(&kfs, 4.0, &0.0, Extrapolation::PingPong, Extrapolation::PingPong);
        assert!((pingpong_val - 200.0).abs() < 1e-3);
    }

    #[test]
    fn test_multi_dimensional_property_interpolation() {
        let fps = 30.0;
        let t0 = TimeCode::from_frames(0, fps);
        let t30 = TimeCode::from_frames(30, fps);

        // Vec2 Position Interpolation
        let mut pos_prop = Property::new("Position", Vec2::new(100.0, 200.0));
        pos_prop.add_keyframe(Keyframe::linear(t0, Vec2::new(100.0, 200.0)));
        pos_prop.add_keyframe(Keyframe::linear(t30, Vec2::new(900.0, 600.0)));

        assert!(pos_prop.is_animated());
        assert_eq!(pos_prop.keyframe_count(), 2);

        let pos_mid = pos_prop.evaluate_at_seconds(0.5);
        assert_eq!(pos_mid, Vec2::new(500.0, 400.0));

        let pos_q = pos_prop.evaluate_at(&TimeCode::from_frames(15, fps));
        assert_eq!(pos_q, Vec2::new(500.0, 400.0));

        // Color RGBA Interpolation
        let mut color_prop = Property::new("Color", Color::RED);
        color_prop.add_keyframe(Keyframe::linear(t0, Color::rgba(1.0, 0.0, 0.0, 1.0)));
        color_prop.add_keyframe(Keyframe::linear(t30, Color::rgba(0.0, 0.0, 1.0, 0.5)));

        let col_mid = color_prop.evaluate_at_seconds(0.5);
        assert!((col_mid.r - 0.5).abs() < 1e-5);
        assert_eq!(col_mid.g, 0.0);
        assert!((col_mid.b - 0.5).abs() < 1e-5);
        assert!((col_mid.a - 0.75).abs() < 1e-5);
    }

    #[test]
    fn test_property_keyframe_management_and_evaluation() {
        let fps = 30.0;
        let mut prop = Property::new("Opacity", 100.0f32);
        assert!(!prop.is_animated());
        assert!(!prop.has_keyframes());

        // Add keyframes in arbitrary/out-of-order temporal order
        prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(60, fps), 100.0));
        prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(0, fps), 0.0));
        prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(30, fps), 50.0));

        // Verify sorted order: frames 0, 30, 60
        assert_eq!(prop.keyframe_count(), 3);
        assert_eq!(prop.keyframes()[0].time.frames(), 0);
        assert_eq!(prop.keyframes()[1].time.frames(), 30);
        assert_eq!(prop.keyframes()[2].time.frames(), 60);

        // Update keyframe at identical timestamp replaces rather than duplicates
        prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(30, fps), 75.0));
        assert_eq!(prop.keyframe_count(), 3);
        assert_eq!(prop.keyframes()[1].value, 75.0);

        // Find keyframe
        let found = prop.keyframe_at(&TimeCode::from_frames(30, fps));
        assert!(found.is_some());
        assert_eq!(found.unwrap().value, 75.0);

        // Remove keyframe
        let removed = prop.remove_keyframe_at(&TimeCode::from_frames(30, fps));
        assert!(removed.is_some());
        assert_eq!(prop.keyframe_count(), 2);

        // Clear keyframes
        prop.clear_keyframes();
        assert!(!prop.is_animated());
        assert_eq!(prop.keyframe_count(), 0);
        assert_eq!(prop.evaluate_at_seconds(1.5), 100.0); // falls back to static value
    }

    #[test]
    fn test_keyframe_serialization_roundtrip_and_backward_compatibility() {
        let fps = 30.0;
        let mut prop = Property::new("Rotation", 0.0f32);
        prop.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(0, fps),
            0.0,
            None,
            Some(KeyframeTangent::new(0.3, 0.1)),
        ));
        prop.add_keyframe(Keyframe::hold(TimeCode::from_frames(30, fps), 90.0));
        prop.add_keyframe(Keyframe::linear(TimeCode::from_frames(60, fps), 180.0));

        // Serialize
        let json = serde_json::to_string_pretty(&prop).expect("Serialize property with keyframes");

        // Deserialize
        let restored: Property<f32> = serde_json::from_str(&json).expect("Deserialize property");
        assert_eq!(prop, restored);
        assert_eq!(restored.keyframe_count(), 3);
        assert_eq!(restored.keyframes()[0].interpolation, KeyframeInterpolation::Bezier);
        assert_eq!(restored.keyframes()[1].interpolation, KeyframeInterpolation::Hold);
        assert_eq!(restored.keyframes()[2].interpolation, KeyframeInterpolation::Linear);

        // Backward compatibility: JSON with NO keyframes field deserializes into empty keyframes
        let legacy_json = r#"{
            "name": "Legacy Scale",
            "value": 100.0,
            "default_value": 100.0,
            "animated": false
        }"#;
        let legacy_prop: Property<f32> = serde_json::from_str(legacy_json).expect("Deserialize legacy property");
        assert_eq!(legacy_prop.name(), "Legacy Scale");
        assert_eq!(*legacy_prop, 100.0);
        assert!(legacy_prop.keyframes.is_empty());
        assert!(!legacy_prop.is_animated());
    }
}
