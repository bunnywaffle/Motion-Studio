//! GPU filter library: every effect as a WGSL snippet.
//!
//! The viewport previews effects on the CPU (`process_color` / the
//! application rasterizer) while export runs on the GPU. This module is
//! the shared contract between the two: each effect has a WGSL function
//! implementing EXACTLY the same math as its CPU twin (same constants,
//! same `/100` scalings, same clamps), and `compose_chain` stitches a
//! list of effects into one fragment-stage filter function.
//!
//! Two snippet kinds exist because geometry and color compose
//! differently:
//! - [`UvFilter`]s remap `uv` before the source is sampled (perspective
//!   skew, tiler mirror, warp field). The composer folds them in listed
//!   order into a single pre-sample transform.
//! - [`ColorFilter`]s map sampled `(uv, color)` to a new color and chain
//!   in listed order afterwards.
//!
//! Spatial passes (gaussian blur, drop-shadow softness, bloom radius)
//! stay multi-tap GPU passes (`BLUR_WGSL`, screen-merge) or CPU
//! convolution in previews; their scalar cores live here. Gaussian blur
//! itself already ships as [`crate::blur::BLUR_WGSL`].
//!
//! Every snippet is validated by [`validate_all`] with the naga WGSL
//! frontend (CPU-only, no adapter needed), and each carries its CPU
//! parity note.

/// A WGSL color-mapping filter: `(uv, color) -> color`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorFilter {
    /// Stable id (`"tint"`, `"exposure"`, ...).
    pub id: &'static str,
    /// WGSL function source (self-contained, no bindings).
    pub wgsl: &'static str,
}

/// A WGSL uv-remap filter: `uv -> uv`, applied pre-sample in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UvFilter {
    /// Stable id (`"perspective"`, `"tiler"`, `"warp"`).
    pub id: &'static str,
    /// WGSL function source (self-contained, no bindings).
    pub wgsl: &'static str,
}

macro_rules! color_filter {
    ($id:literal, $src:literal) => {
        ColorFilter { id: $id, wgsl: $src }
    };
}

macro_rules! uv_filter {
    ($id:literal, $src:literal) => {
        UvFilter { id: $id, wgsl: $src }
    };
}

// ---------------------------------------------------------------------------
// Color filters (mirror `process_color` exactly)
// ---------------------------------------------------------------------------

/// Brightness (-100..100) + contrast (-100..100).
pub fn brightness_contrast() -> ColorFilter {
    color_filter!(
        "brightness_contrast",
        r#"fn fx_brightness_contrast(uv: vec2<f32>, color: vec4<f32>, brightness: f32, contrast: f32) -> vec4<f32> {
    let b = brightness / 100.0;
    let k = max(1.0 + contrast / 100.0, 0.0);
    let rgb = (color.rgb - vec3<f32>(0.5)) * k + vec3<f32>(0.5) + vec3<f32>(b);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Two-color luminance tint with amount (0..100).
pub fn tint() -> ColorFilter {
    color_filter!(
        "tint",
        r#"fn fx_tint(uv: vec2<f32>, color: vec4<f32>, map_black: vec3<f32>, map_white: vec3<f32>, amount: f32) -> vec4<f32> {
    let t = clamp(amount / 100.0, 0.0, 1.0);
    let lum = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let mapped = map_black * (1.0 - lum) + map_white * lum;
    return vec4<f32>(mix(color.rgb, mapped, t), color.a);
}"#
    )
}

/// Channel invert with amount (0..100).
pub fn invert() -> ColorFilter {
    color_filter!(
        "invert",
        r#"fn fx_invert(uv: vec2<f32>, color: vec4<f32>, amount: f32) -> vec4<f32> {
    let t = clamp(amount / 100.0, 0.0, 1.0);
    return vec4<f32>(mix(color.rgb, vec3<f32>(1.0) - color.rgb, t), color.a);
}"#
    )
}

/// Legacy 4-param custom shader grade (matches the CPU approximation).
pub fn custom_grade() -> ColorFilter {
    color_filter!(
        "custom_grade",
        r#"fn fx_custom_grade(uv: vec2<f32>, color: vec4<f32>, p1: f32, p2: f32, p3: f32, p4: f32) -> vec4<f32> {
    let gain = 1.0 + p2 / 100.0;
    let shift = p3 / 100.0;
    var mod_alpha = 1.0;
    if (p4 != 0.0) {
        mod_alpha = clamp(p4 / 100.0, 0.0, 1.0);
    }
    let r = clamp(color.r * gain + shift, 0.0, 1.0);
    let g = clamp(color.g * gain, 0.0, 1.0);
    let b = clamp(color.b * gain - shift * 0.5, 0.0, 1.0);
    return vec4<f32>(r, g, b, clamp(color.a * mod_alpha, 0.0, 1.0));
}"#
    )
}

/// Displacement tint-shift core (CPU parity; true offsets are a pass).
pub fn displacement() -> ColorFilter {
    color_filter!(
        "displacement",
        r#"fn fx_displacement(uv: vec2<f32>, color: vec4<f32>, max_h: f32, max_v: f32) -> vec4<f32> {
    let shift_r = max_h * 0.002;
    let shift_b = max_v * 0.002;
    return vec4<f32>(
        clamp(color.r * (1.0 + shift_r), 0.0, 1.0),
        color.g,
        clamp(color.b * (1.0 - shift_b), 0.0, 1.0),
        color.a
    );
}"#
    )
}

/// Chroma key (distance in RGB, tolerance/feather 0..100).
pub fn chroma_key() -> ColorFilter {
    color_filter!(
        "chroma_key",
        r#"fn fx_chroma_key(uv: vec2<f32>, color: vec4<f32>, key_color: vec3<f32>, tolerance: f32, feather: f32) -> vec4<f32> {
    let dist = length(color.rgb - key_color);
    let tol = max(tolerance / 100.0, 0.01);
    let f = max(feather / 100.0, 0.001);
    if (dist < tol) {
        var alpha_mult = 0.0;
        let edge = max(tol - f, 0.0);
        if (dist >= edge) {
            alpha_mult = clamp((dist - edge) / f, 0.0, 1.0);
        }
        return vec4<f32>(color.rgb, color.a * alpha_mult);
    }
    return color;
}"#
    )
}

/// Luma key (threshold 0..100 as fraction, feather ramp).
pub fn luma_key() -> ColorFilter {
    color_filter!(
        "luma_key",
        r#"fn fx_luma_key(uv: vec2<f32>, color: vec4<f32>, threshold: f32, feather: f32) -> vec4<f32> {
    let lum = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let cut = clamp(threshold / 100.0, 0.0, 1.0);
    let f = max(feather / 100.0, 0.001);
    if (lum < cut) {
        var alpha_mult = 0.0;
        let edge = max(cut - f, 0.0);
        if (lum >= edge) {
            alpha_mult = clamp((lum - edge) / f, 0.0, 1.0);
        }
        return vec4<f32>(color.rgb, color.a * alpha_mult);
    }
    return color;
}"#
    )
}

/// Value-hash film grain (CPU parity; amount 0..100, monochrome flag).
pub fn noise() -> ColorFilter {
    color_filter!(
        "noise",
        r#"fn fx_noise_hash(n: f32) -> f32 {
    return fract(sin(n) * 43758.5453);
}
fn fx_noise(uv: vec2<f32>, color: vec4<f32>, amount: f32, monochrome: f32) -> vec4<f32> {
    let k = clamp(amount / 100.0, 0.0, 1.0);
    if (monochrome > 0.5) {
        let n = fx_noise_hash(color.r * 12.9898 + color.g * 78.233 + color.b * 45.164) - 0.5;
        return vec4<f32>(clamp(color.rgb + vec3<f32>(n * k), vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
    }
    let nr = fx_noise_hash(color.r * 12.9898) - 0.5;
    let ng = fx_noise_hash(color.g * 78.233) - 0.5;
    let nb = fx_noise_hash(color.b * 45.164) - 0.5;
    return vec4<f32>(clamp(color.rgb + vec3<f32>(nr, ng, nb) * k, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Checkerboard generator (uv-space cells).
pub fn checkerboard() -> ColorFilter {
    color_filter!(
        "checkerboard",
        r#"fn fx_checkerboard(uv: vec2<f32>, color: vec4<f32>, cells: vec2<f32>, color_a: vec3<f32>, color_b: vec3<f32>, alpha: f32) -> vec4<f32> {
    let cell = vec2<i32>(i32(floor(uv.x * cells.x)), i32(floor(uv.y * cells.y)));
    let pick_b = (cell.x + cell.y) % 2 == 0;
    let rgb = mix(color_a, color_b, mix(0.0, 1.0, f32(pick_b)));
    return vec4<f32>(rgb, alpha);
}"#
    )
}

/// Linear gradient generator (angle in radians, uv-space).
pub fn gradient_ramp() -> ColorFilter {
    color_filter!(
        "gradient_ramp",
        r#"fn fx_gradient_ramp(uv: vec2<f32>, color: vec4<f32>, color_a: vec3<f32>, color_b: vec3<f32>, angle: f32, alpha: f32) -> vec4<f32> {
    let d = vec2<f32>(cos(angle), sin(angle));
    let t = clamp(dot(uv - vec2<f32>(0.5), d) + 0.5, 0.0, 1.0);
    return vec4<f32>(mix(color_a, color_b, t), alpha);
}"#
    )
}

/// Highlight bloom lift (radius stays a separate blur pass).
pub fn bloom() -> ColorFilter {
    color_filter!(
        "bloom",
        r#"fn fx_bloom(uv: vec2<f32>, color: vec4<f32>, glow: vec3<f32>, intensity: f32) -> vec4<f32> {
    let k = clamp(intensity / 100.0, 0.0, 1.0);
    let lifted = vec3<f32>(1.0) - (vec3<f32>(1.0) - color.rgb) * (vec3<f32>(1.0) - glow * k);
    return vec4<f32>(clamp(lifted, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Exposure gain in EV stops.
pub fn exposure() -> ColorFilter {
    color_filter!(
        "exposure",
        r#"fn fx_exposure(uv: vec2<f32>, color: vec4<f32>, ev: f32) -> vec4<f32> {
    let gain = exp2(clamp(ev, -10.0, 10.0));
    return vec4<f32>(clamp(color.rgb * gain, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Vibrance: saturation weighted toward muted colors.
pub fn vibrance() -> ColorFilter {
    color_filter!(
        "vibrance",
        r#"fn fx_vibrance(uv: vec2<f32>, color: vec4<f32>, vibrance: f32) -> vec4<f32> {
    let v = clamp(vibrance / 100.0, -1.0, 1.0);
    let lum = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let sat = clamp(max(color.r, max(color.g, color.b)) - min(color.r, min(color.g, color.b)), 0.0, 1.0);
    let boost = 1.0 + v * (1.0 - sat);
    let rgb = vec3<f32>(lum) + (color.rgb - vec3<f32>(lum)) * boost;
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Drop-shadow alpha core (offset/blur are separate passes).
pub fn drop_shadow_core() -> ColorFilter {
    color_filter!(
        "drop_shadow_core",
        r#"fn fx_drop_shadow_core(uv: vec2<f32>, color: vec4<f32>, shadow_color: vec3<f32>, opacity: f32) -> vec4<f32> {
    let a = clamp(opacity / 100.0, 0.0, 1.0) * 0.75;
    return vec4<f32>(shadow_color, color.a * a);
}"#
    )
}

// ---------------------------------------------------------------------------
// UV filters (pre-sample remaps, applied in listed order)
// ---------------------------------------------------------------------------

/// Skew about uv center (degrees).
pub fn perspective() -> UvFilter {
    uv_filter!(
        "perspective",
        r#"fn fx_uv_perspective(uv: vec2<f32>, skew_x_deg: f32, skew_y_deg: f32) -> vec2<f32> {
    let sx = clamp(tan(radians(clamp(skew_x_deg, -60.0, 60.0))), -2.0, 2.0);
    let sy = clamp(tan(radians(clamp(skew_y_deg, -60.0, 60.0))), -2.0, 2.0);
    let p = uv - vec2<f32>(0.5);
    return vec2<f32>(p.x + sx * p.y, sy * p.x + p.y) + vec2<f32>(0.5);
}"#
    )
}

/// Mirror-tile the uv domain.
pub fn tiler() -> UvFilter {
    uv_filter!(
        "tiler",
        r#"fn fx_uv_tiler(uv: vec2<f32>, tiles_x: f32, tiles_y: f32) -> vec2<f32> {
    let tx = clamp(floor(tiles_x), 1.0, 32.0);
    let ty = clamp(floor(tiles_y), 1.0, 32.0);
    return fract(uv * vec2<f32>(tx, ty));
}"#
    )
}

/// Sine-field warp of uv.
pub fn warp() -> UvFilter {
    uv_filter!(
        "warp",
        r#"fn fx_uv_warp(uv: vec2<f32>, amount: f32, scale: f32) -> vec2<f32> {
    let amp = clamp(amount / 100.0, 0.0, 1.0) * 0.25;
    let f = clamp(scale, 0.1, 10.0) * 6.2831;
    let ox = sin(uv.y * f) * amp;
    let oy = sin(uv.x * f * 1.3 + 1.7) * amp;
    return fract(uv + vec2<f32>(ox, oy));
}"#
    )
}

// ---------------------------------------------------------------------------
// Registry + composer
// ---------------------------------------------------------------------------

/// All color filters in stable order.
pub fn all_color_filters() -> Vec<ColorFilter> {
    vec![
        brightness_contrast(),
        tint(),
        invert(),
        custom_grade(),
        displacement(),
        chroma_key(),
        luma_key(),
        noise(),
        checkerboard(),
        gradient_ramp(),
        bloom(),
        exposure(),
        vibrance(),
        drop_shadow_core(),
    ]
}

/// All uv filters in stable order.
pub fn all_uv_filters() -> Vec<UvFilter> {
    vec![perspective(), tiler(), warp()]
}

/// Stitch uv remaps + color calls into one fragment-stage function.
/// `color_calls` are full WGSL statements assigning `color` (e.g.
/// `"color = fx_tint(uv, color, ...);"`); `uv_calls` assign `uv`.
pub fn compose_chain(uv_calls: &[&str], color_calls: &[&str]) -> String {
    let mut src = String::from(
        "fn apply_effects(uv_in: vec2<f32>, color_in: vec4<f32>) -> vec4<f32> {\n    var uv = uv_in;\n",
    );
    for call in uv_calls {
        src.push_str("    uv = ");
        src.push_str(call);
        src.push_str(";\n");
    }
    src.push_str("    var color = color_in;\n");
    for call in color_calls {
        src.push_str("    color = ");
        src.push_str(call);
        src.push_str(";\n");
    }
    src.push_str("    return color;\n}\n");
    src
}

/// Validate every snippet plus a representative composed chain with the
/// naga WGSL frontend (CPU-only).
pub fn validate_all() -> Result<(), String> {
    for f in all_color_filters() {
        naga::front::wgsl::parse_str(f.wgsl)
            .map_err(|e| format!("filter {} invalid: {e:?}", f.id))?;
    }
    for f in all_uv_filters() {
        naga::front::wgsl::parse_str(f.wgsl)
            .map_err(|e| format!("uv filter {} invalid: {e:?}", f.id))?;
    }
    let mut chain_src = String::new();
    for f in all_color_filters() {
        chain_src.push_str(f.wgsl);
        chain_src.push('\n');
    }
    for f in all_uv_filters() {
        chain_src.push_str(f.wgsl);
        chain_src.push('\n');
    }
    chain_src.push_str(&compose_chain(
        &["fx_uv_tiler(uv, 2.0, 2.0)", "fx_uv_warp(uv, 30.0, 1.0)"],
        &[
            "fx_tint(uv, color, vec3<f32>(0.0), vec3<f32>(1.0), 80.0)",
            "fx_exposure(uv, color, 1.0)",
            "fx_vibrance(uv, color, 30.0)",
            "fx_bloom(uv, color, vec3<f32>(0.4), 40.0)",
        ],
    ));
    naga::front::wgsl::parse_str(&chain_src).map_err(|e| format!("composed chain invalid: {e:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_snippets_validate() {
        validate_all().expect("all WGSL filters must parse");
    }

    #[test]
    fn registry_covers_effect_types() {
        let ids: Vec<&str> = all_color_filters().iter().map(|f| f.id).collect();
        for want in [
            "brightness_contrast",
            "tint",
            "invert",
            "custom_grade",
            "displacement",
            "chroma_key",
            "luma_key",
            "noise",
            "checkerboard",
            "gradient_ramp",
            "bloom",
            "exposure",
            "vibrance",
            "drop_shadow_core",
        ] {
            assert!(ids.contains(&want), "missing {want}");
        }
        let uids: Vec<&str> = all_uv_filters().iter().map(|f| f.id).collect();
        for want in ["perspective", "tiler", "warp"] {
            assert!(uids.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn composer_emits_ordered_chain() {
        let src = compose_chain(&["fx_uv_tiler(uv, 2.0, 2.0)"], &["fx_exposure(uv, color, 1.0)"]);
        let uv_pos = src.find("fx_uv_tiler").unwrap();
        let col_pos = src.find("fx_exposure").unwrap();
        assert!(uv_pos < col_pos);
        naga::front::wgsl::parse_str(&format!(
            "{}\n{}\n{src}",
            tiler().wgsl,
            exposure().wgsl
        ))
        .expect("chain parses");
    }
}
