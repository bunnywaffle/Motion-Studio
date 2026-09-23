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

/// Displacement tint-shift core (legacy preview approximation; true
/// luminance-driven displacement is a raster resample pass).
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

/// Levels: input-range remap + gamma + output range (CPU parity).
pub fn levels() -> ColorFilter {
    color_filter!(
        "levels",
        r#"fn fx_levels(uv: vec2<f32>, color: vec4<f32>, in_black: f32, in_white: f32, gamma: f32, out_black: f32, out_white: f32) -> vec4<f32> {
    let ib = clamp(in_black / 255.0, 0.0, 1.0);
    let iw = clamp(in_white / 255.0, 0.0, 1.0);
    let ob = clamp(out_black / 255.0, 0.0, 1.0);
    let ow = clamp(out_white / 255.0, 0.0, 1.0);
    let span = max(iw - ib, 0.001);
    let t = clamp((color.rgb - vec3<f32>(ib)) / vec3<f32>(span), vec3<f32>(0.0), vec3<f32>(1.0));
    let g = pow(t, vec3<f32>(1.0 / clamp(gamma, 0.1, 9.9)));
    let rgb = clamp(vec3<f32>(ob) + g * (vec3<f32>(ow) - vec3<f32>(ob)), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, color.a);
}"#
    )
}

/// Hue shift + saturation scale + lightness offset (CPU parity).
pub fn hue_saturation() -> ColorFilter {
    color_filter!(
        "hue_saturation",
        r#"fn fx_hsl_hue(h: f32, s: f32, l: f32, n: f32) -> f32 {
    let k = (n + h * 12.0) % 12.0;
    let a = s * min(l, 1.0 - l);
    return l - a * max(-1.0, min(min(k - 3.0, 9.0 - k), 1.0));
}
fn fx_hue_saturation(uv: vec2<f32>, color: vec4<f32>, hue_shift: f32, saturation: f32, lightness: f32) -> vec4<f32> {
    let mx = max(color.r, max(color.g, color.b));
    let mn = min(color.r, min(color.g, color.b));
    var h = 0.0;
    var s = 0.0;
    let l = (mx + mn) * 0.5;
    let d = mx - mn;
    if (d > 0.000001) {
        if (l > 0.5) {
            s = d / max(2.0 - mx - mn, 0.000001);
        } else {
            s = d / max(mx + mn, 0.000001);
        }
        if (mx == color.r) {
            h = (color.g - color.b) / d;
            if (color.g < color.b) {
                h = h + 6.0;
            }
        } else if (mx == color.g) {
            h = (color.b - color.r) / d + 2.0;
        } else {
            h = (color.r - color.g) / d + 4.0;
        }
        h = h / 6.0;
    }
    let h2 = (h + hue_shift / 360.0) % 1.0;
    let s2 = clamp(s * (1.0 + saturation / 100.0), 0.0, 1.0);
    let l2 = clamp(l + lightness / 100.0, 0.0, 1.0);
    let hh = (h2 + 1.0) % 1.0;
    let r = fx_hsl_hue(hh, s2, l2, 0.0);
    let g = fx_hsl_hue(hh, s2, l2, 8.0);
    let b = fx_hsl_hue(hh, s2, l2, 4.0);
    return vec4<f32>(clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Edge vignette (CPU parity; needs uv, hence a color filter).
pub fn vignette() -> ColorFilter {
    color_filter!(
        "vignette",
        r#"fn fx_vignette(uv: vec2<f32>, color: vec4<f32>, amount: f32, softness: f32) -> vec4<f32> {
    let k = clamp(amount / 100.0, 0.0, 1.0);
    let soft = clamp(softness / 100.0, 0.0, 1.0);
    let inner = 0.5 * (1.0 - soft * 0.85);
    let outer = 0.5 + 0.28 * (1.0 - soft * 0.4);
    let d = length((uv - vec2<f32>(0.5)) * 2.0) / 1.41421356;
    let t = clamp((d - inner) / max(outer - inner, 0.001), 0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    return vec4<f32>(color.rgb * (1.0 - k * s), color.a);
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
// Stock plug-in twins (CPU parity with compositor::fx / raster::stock)
// ---------------------------------------------------------------------------

/// 5-zone lift curve (shadows, darks, mids, lights, highlights in -100..100).
pub fn stock_curves() -> ColorFilter {
    color_filter!(
        "stock_curves",
        r#"fn fx_stock_curves(uv: vec2<f32>, color: vec4<f32>, lifts: vec4<f32>, hi: f32) -> vec4<f32> {
    let zones = vec4<f32>(0.1, 0.3, 0.5, 0.7);
    var rgb = color.rgb;
    rgb += lifts.x / 100.0 * 0.6 * clamp(1.0 - abs(rgb - vec3<f32>(zones.x)) / vec3<f32>(0.35), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb += lifts.y / 100.0 * 0.6 * clamp(1.0 - abs(rgb - vec3<f32>(zones.y)) / vec3<f32>(0.35), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb += lifts.z / 100.0 * 0.6 * clamp(1.0 - abs(rgb - vec3<f32>(zones.z)) / vec3<f32>(0.35), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb += lifts.w / 100.0 * 0.6 * clamp(1.0 - abs(rgb - vec3<f32>(zones.w)) / vec3<f32>(0.35), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb += hi / 100.0 * 0.6 * clamp(1.0 - abs(rgb - vec3<f32>(0.9)) / vec3<f32>(0.35), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// CMY color balance with midtone weighting.
pub fn stock_color_balance() -> ColorFilter {
    color_filter!(
        "stock_color_balance",
        r#"fn fx_stock_color_balance(uv: vec2<f32>, color: vec4<f32>, balance: vec3<f32>) -> vec4<f32> {
    let m = clamp(sin(color.rgb * 3.14159265), vec3<f32>(0.0), vec3<f32>(1.0));
    var rgb = color.rgb;
    let cr = balance.x / 100.0;
    let mg = balance.y / 100.0;
    let yb = balance.z / 100.0;
    if (cr >= 0.0) { rgb.r += cr * m.r * 0.5; } else { rgb.g += -cr * m.g * 0.25; rgb.b += -cr * m.b * 0.25; }
    if (mg >= 0.0) { rgb.g += mg * m.g * 0.5; } else { rgb.r += -mg * m.r * 0.25; rgb.b += -mg * m.b * 0.25; }
    if (yb >= 0.0) { rgb.b += yb * m.b * 0.5; } else { rgb.r += -yb * m.r * 0.25; rgb.g += -yb * m.g * 0.25; }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Shadows / midtones / highlights lift.
pub fn stock_color_wheels() -> ColorFilter {
    color_filter!(
        "stock_color_wheels",
        r#"fn fx_stock_color_wheels(uv: vec2<f32>, color: vec4<f32>, wheels: vec3<f32>) -> vec4<f32> {
    let ws = (1.0 - color.rgb) * (1.0 - color.rgb);
    let wm = 1.0 - (2.0 * color.rgb - 1.0) * (2.0 * color.rgb - 1.0);
    let wh = color.rgb * color.rgb;
    let rgb = color.rgb + wheels.x / 100.0 * ws * 0.5 + wheels.y / 100.0 * wm * 0.5 + wheels.z / 100.0 * wh * 0.5;
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Temperature / tint grade.
pub fn stock_temperature_tint() -> ColorFilter {
    color_filter!(
        "stock_temperature_tint",
        r#"fn fx_stock_temperature_tint(uv: vec2<f32>, color: vec4<f32>, temp_tint: vec2<f32>) -> vec4<f32> {
    let t = temp_tint.x / 100.0;
    let ti = temp_tint.y / 100.0;
    let rgb = vec3<f32>(color.r + t * 0.35 - ti * 0.10, color.g + ti * 0.15, color.b - t * 0.35 - ti * 0.10);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Posterize to N levels.
pub fn stock_posterize() -> ColorFilter {
    color_filter!(
        "stock_posterize",
        r#"fn fx_stock_posterize(uv: vec2<f32>, color: vec4<f32>, levels: f32) -> vec4<f32> {
    let n = clamp(floor(levels), 2.0, 32.0);
    let rgb = floor(color.rgb * (n - 1.0) + 0.5) / (n - 1.0);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Threshold with feather.
pub fn stock_threshold() -> ColorFilter {
    color_filter!(
        "stock_threshold",
        r#"fn fx_stock_threshold(uv: vec2<f32>, color: vec4<f32>, threshold: f32, feather: f32) -> vec4<f32> {
    let t = clamp(threshold / 100.0, 0.0, 1.0);
    let f = max(feather / 100.0, 0.001);
    let l = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    var s = clamp((l - t) / f + 0.5, 0.0, 1.0);
    s = s * s * (3.0 - 2.0 * s);
    return vec4<f32>(vec3<f32>(s), color.a);
}"#
    )
}

/// Luminance-difference key.
pub fn stock_difference_key() -> ColorFilter {
    color_filter!(
        "stock_difference_key",
        r#"fn fx_stock_difference_key(uv: vec2<f32>, color: vec4<f32>, key_luma: f32, threshold: f32, feather: f32) -> vec4<f32> {
    let key = clamp(key_luma / 100.0, 0.0, 1.0);
    let th = clamp(threshold / 100.0, 0.0, 1.0);
    let f = max(feather / 100.0, 0.001);
    let d = abs(dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114)) - key);
    var a = 1.0;
    if (d < th) {
        a = 0.0;
    } else {
        a = clamp((d - th) / f, 0.0, 1.0);
    }
    return vec4<f32>(color.rgb, color.a * a);
}"#
    )
}

/// Green spill suppression.
pub fn stock_spill_suppress() -> ColorFilter {
    color_filter!(
        "stock_spill_suppress",
        r#"fn fx_stock_spill_suppress(uv: vec2<f32>, color: vec4<f32>, amount: f32) -> vec4<f32> {
    let a = clamp(amount / 100.0, 0.0, 1.0);
    let cap = (color.r + color.b) * 0.5;
    return vec4<f32>(color.r, mix(color.g, min(color.g, cap), a), color.b, color.a);
}"#
    )
}

/// Warm diagonal light leak.
pub fn stock_light_leak() -> ColorFilter {
    color_filter!(
        "stock_light_leak",
        r#"fn fx_stock_light_leak(uv: vec2<f32>, color: vec4<f32>, intensity: f32, hue_shift: f32, position: f32) -> vec4<f32> {
    let k = clamp(intensity / 100.0, 0.0, 1.0);
    let ox = clamp(position / 100.0, 0.0, 1.0);
    let d = length(vec2<f32>(uv.x - ox, uv.y + 0.1));
    let m = pow(clamp(1.0 - d * 1.6, 0.0, 1.0), 1.5) * k;
    let warm = vec3<f32>(1.0, 0.45 + hue_shift / 360.0, 0.15);
    let rgb = vec3<f32>(1.0) - (vec3<f32>(1.0) - color.rgb) * (vec3<f32>(1.0) - clamp(warm, vec3<f32>(0.0), vec3<f32>(1.0)) * m);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Scanlines darkening.
pub fn stock_scanlines() -> ColorFilter {
    color_filter!(
        "stock_scanlines",
        r#"fn fx_stock_scanlines(uv: vec2<f32>, color: vec4<f32>, size: f32, intensity: f32, res_y: f32) -> vec4<f32> {
    let row = floor(uv.y * res_y / clamp(size, 1.0, 16.0));
    var m = 1.0;
    if (fract(row * 0.5) > 0.25) {
        m = 1.0 - clamp(intensity / 100.0, 0.0, 1.0) * 0.85;
    }
    return vec4<f32>(color.rgb * m, color.a);
}"#
    )
}

/// Animated film grain (hash per uv cell + time).
pub fn stock_film_grain() -> ColorFilter {
    color_filter!(
        "stock_film_grain",
        r#"fn fx_hash21(p: vec2<f32>, seed: f32) -> f32 {
    let h = fract(sin(dot(p, vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    return h;
}
fn fx_stock_film_grain(uv: vec2<f32>, color: vec4<f32>, amount: f32, size: f32, time: f32) -> vec4<f32> {
    let k = clamp(amount / 100.0, 0.0, 1.0);
    let cell = floor(uv * vec2<f32>(480.0) / max(size, 1.0));
    let n = (fx_hash21(cell, floor(time * 24.0) * 0.913 + 2.0) - 0.5) * k;
    return vec4<f32>(clamp(color.rgb + vec3<f32>(n), vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Static fractal detail blended over content.
pub fn stock_fractal_noise() -> ColorFilter {
    color_filter!(
        "stock_fractal_noise",
        r#"fn fx_vnoise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = fract(sin(dot(i, vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let b = fract(sin(dot(i + vec2<f32>(1.0, 0.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let c = fract(sin(dot(i + vec2<f32>(0.0, 1.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let d = fract(sin(dot(i + vec2<f32>(1.0, 1.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn fx_stock_fractal_noise(uv: vec2<f32>, color: vec4<f32>, scale: f32, seed: f32) -> vec4<f32> {
    var v = 0.0;
    var amp = 0.5;
    var p = uv * 8.0 * clamp(scale, 0.1, 10.0);
    for (var i = 0; i < 4; i += 1) {
        v += amp * fx_vnoise(p, seed + f32(i) * 13.7);
        amp *= 0.5;
        p = p * 2.03 + 17.3;
    }
    let m = (clamp(v, 0.0, 1.0) - 0.5) * 0.6;
    return vec4<f32>(clamp(color.rgb + vec3<f32>(m), vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Sparse dust specks.
pub fn stock_dust() -> ColorFilter {
    color_filter!(
        "stock_dust",
        r#"fn fx_stock_dust(uv: vec2<f32>, color: vec4<f32>, amount: f32, size: f32, time: f32) -> vec4<f32> {
    let cell = floor(uv * vec2<f32>(120.0));
    let h = fract(sin(dot(cell, vec2<f32>(12.9898, 78.233)) + floor(time * 24.0) * 0.37) * 43758.5453);
    if (h > 1.0 - clamp(amount / 100.0, 0.0, 1.0) * 0.12) {
        let lp = fract(uv * vec2<f32>(120.0)) - 0.5;
        if (length(lp) * 9.0 < clamp(size, 1.0, 12.0) * 0.5) {
            let v = 0.05;
            return vec4<f32>(vec3<f32>(v), color.a);
        }
    }
    return color;
}"#
    )
}

/// Vertical scratch lines.
pub fn stock_scratches() -> ColorFilter {
    color_filter!(
        "stock_scratches",
        r#"fn fx_stock_scratches(uv: vec2<f32>, color: vec4<f32>, amount: f32, time: f32) -> vec4<f32> {
    let col = floor(uv.x * 480.0);
    let h = fract(sin(col * 12.9898 + floor(time * 24.0) * 0.53) * 43758.5453);
    if (h > 1.0 - clamp(amount / 100.0, 0.0, 1.0) * 0.10) {
        return vec4<f32>(mix(color.rgb, vec3<f32>(0.9), 0.75), color.a);
    }
    return color;
}"#
    )
}

/// Global flicker gain.
pub fn stock_flicker() -> ColorFilter {
    color_filter!(
        "stock_flicker",
        r#"fn fx_stock_flicker(uv: vec2<f32>, color: vec4<f32>, amount: f32, speed: f32, time: f32) -> vec4<f32> {
    let t = time * (0.5 + clamp(speed, 0.0, 100.0) * 2.0 / 100.0);
    let h = fract(sin(floor(t) * 12.9898 + 0.5) * 43758.5453);
    let g = 1.0 + (h - 0.5) * 2.0 * clamp(amount / 100.0, 0.0, 1.0) * 0.6;
    return vec4<f32>(clamp(color.rgb * g, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}"#
    )
}

/// Dot-screen halftone.
pub fn stock_halftone() -> ColorFilter {
    color_filter!(
        "stock_halftone",
        r#"fn fx_stock_halftone(uv: vec2<f32>, color: vec4<f32>, size: f32, angle: f32, res: vec2<f32>) -> vec4<f32> {
    let l = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let a = radians(clamp(angle, 0.0, 90.0));
    let p = uv * res / clamp(size, 2.0, 32.0);
    let pr = vec2<f32>(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a));
    let cell = length(fract(pr) - 0.5) * 2.0;
    var dot_v = 0.06;
    if (cell < (1.0 - l) * 1.1) {
        dot_v = 1.0;
    }
    return vec4<f32>(vec3<f32>(dot_v), color.a);
}"#
    )
}

/// Flat color fill.
pub fn stock_solid() -> ColorFilter {
    color_filter!(
        "stock_solid",
        r#"fn fx_stock_solid(uv: vec2<f32>, color: vec4<f32>, fill: vec3<f32>, opacity: f32) -> vec4<f32> {
    let op = clamp(opacity / 100.0, 0.0, 1.0);
    return vec4<f32>(fill * op, op);
}"#
    )
}

/// Fractal gradient fill between two colors.
pub fn stock_fractal_gen() -> ColorFilter {
    color_filter!(
        "stock_fractal_gen",
        r#"fn fx_sfg_vnoise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = fract(sin(dot(i, vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let b = fract(sin(dot(i + vec2<f32>(1.0, 0.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let c = fract(sin(dot(i + vec2<f32>(0.0, 1.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let d = fract(sin(dot(i + vec2<f32>(1.0, 1.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn fx_stock_fractal_gen(uv: vec2<f32>, color: vec4<f32>, col_a: vec3<f32>, col_b: vec3<f32>, scale: f32, seed: f32, opacity: f32) -> vec4<f32> {
    var v = 0.0;
    var amp = 0.5;
    var p = uv * 8.0 * clamp(scale, 0.1, 10.0);
    for (var i = 0; i < 4; i += 1) {
        v += amp * fx_sfg_vnoise(p, seed + f32(i) * 13.7);
        amp *= 0.5;
        p = p * 2.03 + 17.3;
    }
    let n = clamp(v, 0.0, 1.0);
    let op = clamp(opacity / 100.0, 0.0, 1.0);
    return vec4<f32>((col_a + (col_b - col_a) * n) * op, op);
}"#
    )
}

/// Grid lines generator.
pub fn stock_grid() -> ColorFilter {
    color_filter!(
        "stock_grid",
        r#"fn fx_stock_grid(uv: vec2<f32>, color: vec4<f32>, fill: vec3<f32>, size: f32, line: f32, opacity: f32, res: vec2<f32>) -> vec4<f32> {
    let p = uv * res;
    let sz = clamp(size, 2.0, 256.0);
    let mx = p.x - floor(p.x / sz) * sz;
    let my = p.y - floor(p.y / sz) * sz;
    let fx = min(mx, sz - mx);
    let fy = min(my, sz - my);
    let op = clamp(opacity / 100.0, 0.0, 1.0);
    if (fx < clamp(line, 1.0, 32.0) || fy < clamp(line, 1.0, 32.0)) {
        return vec4<f32>(fill * op, op);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}"#
    )
}

/// Circle / rect / ring shape generator.
pub fn stock_shapes() -> ColorFilter {
    color_filter!(
        "stock_shapes",
        r#"fn fx_stock_shapes(uv: vec2<f32>, color: vec4<f32>, fill: vec3<f32>, shape: f32, size: f32, softness: f32, opacity: f32) -> vec4<f32> {
    let p = abs(uv - 0.5) * 2.0;
    let r = clamp(size / 100.0, 0.0, 1.0);
    let feather = max(clamp(softness / 100.0, 0.0, 1.0) * r * 0.5 + 0.01, 0.01);
    var d = length(p) - r;
    if (shape > 0.5 && shape < 1.5) {
        d = max(p.x, p.y) - r;
    } else if (shape >= 1.5) {
        d = abs(length(p) - r * 0.6) - r * 0.18;
    }
    let a = clamp((feather - d) / (2.0 * feather), 0.0, 1.0) * clamp(opacity / 100.0, 0.0, 1.0);
    return vec4<f32>(fill * a, a);
}"#
    )
}

/// Animated plasma palette fill.
pub fn stock_plasma() -> ColorFilter {
    color_filter!(
        "stock_plasma",
        r#"fn fx_pl_noise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = fract(sin(dot(i, vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let b = fract(sin(dot(i + vec2<f32>(1.0, 0.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let c = fract(sin(dot(i + vec2<f32>(0.0, 1.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    let d = fract(sin(dot(i + vec2<f32>(1.0, 1.0), vec2<f32>(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn fx_pl_fbm(p: vec2<f32>, seed: f32) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 4; i += 1) {
        v += amp * fx_pl_noise(q, seed + f32(i) * 13.7);
        amp *= 0.5;
        q = q * 2.03 + 17.3;
    }
    return clamp(v, 0.0, 1.0);
}
fn fx_stock_plasma(uv: vec2<f32>, color: vec4<f32>, col_a: vec3<f32>, col_b: vec3<f32>, scale: f32, time: f32, opacity: f32) -> vec4<f32> {
    let p = uv * 6.0 * clamp(scale, 0.1, 10.0);
    let v = fx_pl_fbm(p + time * 0.1, 1.0) * 0.6 + fx_pl_fbm(p * 1.7 - time * 0.1, 7.0) * 0.4;
    let m = 0.5 + 0.5 * cos(clamp(v, 0.0, 1.0) * 6.2831853);
    let op = clamp(opacity / 100.0, 0.0, 1.0);
    return vec4<f32>((col_a + (col_b - col_a) * m) * op, op);
}"#
    )
}

/// Procedural particle dots (grid-hashed, twinkling).
pub fn stock_particles() -> ColorFilter {
    color_filter!(
        "stock_particles",
        r#"fn fx_stock_particles(uv: vec2<f32>, color: vec4<f32>, fill: vec3<f32>, density: f32, time: f32) -> vec4<f32> {
    let g = vec2<f32>(48.0, 27.0) * clamp(density, 1.0, 10.0) / 10.0;
    let cell = floor(uv * g);
    let h = fract(sin(dot(cell, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    if (h > 0.45) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let c = fract(uv * g) - 0.5;
    let tw = 0.5 + 0.5 * sin(time * 2.0 + h * 40.0);
    let d = length(c);
    let a = clamp((0.35 - d) / 0.35, 0.0, 1.0) * (0.4 + 0.6 * tw);
    return vec4<f32>(fill * a, a);
}"#
    )
}

/// Rectangular crop (transparent outside).
pub fn stock_crop() -> ColorFilter {
    color_filter!(
        "stock_crop",
        r#"fn fx_stock_crop(uv: vec2<f32>, color: vec4<f32>, crop: vec4<f32>) -> vec4<f32> {
    if (uv.x < crop.x / 100.0 || uv.y < crop.y / 100.0 || uv.x > 1.0 - crop.z / 100.0 || uv.y > 1.0 - crop.w / 100.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return color;
}"#
    )
}

/// Block color quantization (mosaic color step).
pub fn stock_mosaic() -> ColorFilter {
    color_filter!(
        "stock_mosaic",
        r#"fn fx_stock_mosaic(uv: vec2<f32>, color: vec4<f32>, levels: f32) -> vec4<f32> {
    let n = clamp(floor(levels), 2.0, 16.0);
    let rgb = floor(color.rgb * (n - 1.0) + 0.5) / (n - 1.0);
    return vec4<f32>(rgb, color.a);
}"#
    )
}

// ---------------------------------------------------------------------------
// Stock UV twins (pre-sample remaps, same math as raster::stock)
// ---------------------------------------------------------------------------

/// Axis mirror about a center fraction.
pub fn stock_mirror() -> UvFilter {
    uv_filter!(
        "stock_mirror",
        r#"fn fx_uv_stock_mirror(uv: vec2<f32>, mode: f32, center: f32) -> vec2<f32> {
    let c = clamp(center / 100.0, 0.0, 1.0);
    var out = uv;
    if (mode < 0.5 || (mode > 1.5 && mode < 2.5)) {
        out.x = 2.0 * c - uv.x;
    }
    if ((mode > 0.5 && mode < 1.5) || mode >= 1.5) {
        out.y = 2.0 * c - uv.y;
    }
    return out;
}"#
    )
}

/// Wrapping repeat (vs mirror tiling).
pub fn stock_repeat() -> UvFilter {
    uv_filter!(
        "stock_repeat",
        r#"fn fx_uv_stock_repeat(uv: vec2<f32>, tiles: vec2<f32>) -> vec2<f32> {
    return fract(uv * max(floor(tiles), vec2<f32>(1.0)));
}"#
    )
}

/// Wrapping offset (fractions of the frame).
pub fn stock_offset() -> UvFilter {
    uv_filter!(
        "stock_offset",
        r#"fn fx_uv_stock_offset(uv: vec2<f32>, shift: vec2<f32>) -> vec2<f32> {
    return fract(uv - shift / 100.0);
}"#
    )
}

/// Sine wave displacement.
pub fn stock_wave() -> UvFilter {
    uv_filter!(
        "stock_wave",
        r#"fn fx_uv_stock_wave(uv: vec2<f32>, amplitude: f32, wavelength: f32, direction: f32, res: vec2<f32>) -> vec2<f32> {
    let rad = radians(direction);
    let d = vec2<f32>(cos(rad), sin(rad));
    let ph = dot(uv * res, d) * 6.2831853 / max(wavelength, 2.0);
    let o = sin(ph) * amplitude / max(res.x, 1.0);
    return uv + vec2<f32>(-d.y * o, d.x * o);
}"#
    )
}

/// Centered ripple rings.
pub fn stock_ripple() -> UvFilter {
    uv_filter!(
        "stock_ripple",
        r#"fn fx_uv_stock_ripple(uv: vec2<f32>, amplitude: f32, wavelength: f32, res: vec2<f32>) -> vec2<f32> {
    let aspect = vec2<f32>(res.x / max(res.y, 1.0), 1.0);
    let p = (uv - 0.5) * aspect;
    let r = max(length(p), 0.0001);
    let off = sin(r * max(res.y, 1.0) * 6.2831853 / max(wavelength, 2.0)) * amplitude / max(res.y, 1.0) * exp(-r * 1.33);
    return uv - (p / r) * off / aspect;
}"#
    )
}

/// Twirl about the center.
pub fn stock_twirl() -> UvFilter {
    uv_filter!(
        "stock_twirl",
        r#"fn fx_uv_stock_twirl(uv: vec2<f32>, angle: f32, radius: f32) -> vec2<f32> {
    let p = uv - 0.5;
    let r = length(p) * 2.0;
    let rmax = clamp(radius / 100.0, 0.02, 1.0);
    if (r > rmax || r < 0.00001) {
        return uv;
    }
    let th = atan(p.y, p.x) + radians(angle) * (1.0 - r / rmax);
    let rn = r;
    return vec2<f32>(0.5) + vec2<f32>(cos(th), sin(th)) * rn * 0.5;
}"#
    )
}

/// Dome bulge (positive) or pinch (negative).
pub fn stock_bulge() -> UvFilter {
    uv_filter!(
        "stock_bulge",
        r#"fn fx_uv_stock_bulge(uv: vec2<f32>, amount: f32, radius: f32) -> vec2<f32> {
    let p = uv - 0.5;
    let r = length(p) * 2.0;
    let rmax = clamp(radius / 100.0, 0.02, 1.0);
    if (r > rmax || r < 0.00001) {
        return uv;
    }
    let t = r / rmax;
    let rn = (t + amount / 100.0 * (1.0 - t * t) * 0.35 * (1.0 - t)) * rmax;
    return vec2<f32>(0.5) + (p / max(r, 0.00001)) * rn * 0.5;
}"#
    )
}

/// Spherical lens profile.
pub fn stock_spherize() -> UvFilter {
    uv_filter!(
        "stock_spherize",
        r#"fn fx_uv_stock_spherize(uv: vec2<f32>, amount: f32, radius: f32) -> vec2<f32> {
    let p = uv - 0.5;
    let r = length(p) * 2.0;
    let rmax = clamp(radius / 100.0, 0.02, 1.0);
    if (r > rmax || r < 0.00001) {
        return uv;
    }
    let t = r / rmax;
    let rn = t * (1.0 - amount / 100.0 * 0.45 * (1.0 - t * t)) * rmax;
    return vec2<f32>(0.5) + (p / max(r, 0.00001)) * rn * 0.5;
}"#
    )
}

/// Barrel / pincushion lens distortion with zoom.
pub fn stock_lens_distortion() -> UvFilter {
    uv_filter!(
        "stock_lens_distortion",
        r#"fn fx_uv_stock_lens_distortion(uv: vec2<f32>, amount: f32, zoom: f32) -> vec2<f32> {
    let p = uv - 0.5;
    let r2 = dot(p, p) * 4.0;
    let s = 1.0 / max(1.0 + amount / 100.0 * r2, 0.2) / max(zoom / 100.0, 0.5);
    return vec2<f32>(0.5) + p * s;
}"#
    )
}

/// Block downsample coordinates.
pub fn stock_pixelate() -> UvFilter {
    uv_filter!(
        "stock_pixelate",
        r#"fn fx_uv_stock_pixelate(uv: vec2<f32>, size: f32, res: vec2<f32>) -> vec2<f32> {
    let cell = max(size, 1.0) / res;
    return (floor(uv / cell) + 0.5) * cell;
}"#
    )
}

/// Projective corner pin (unit quad corners as separate vecs).
pub fn stock_corner_pin() -> UvFilter {
    uv_filter!(
        "stock_corner_pin",
        r#"fn fx_uv_stock_corner_pin(uv: vec2<f32>, ul: vec2<f32>, ur: vec2<f32>, lr: vec2<f32>, ll: vec2<f32>) -> vec2<f32> {
    // Bilinear patch through the pinned quad (stable for moderate pins).
    let top = mix(ul, ur, uv.x);
    let bot = mix(ll, lr, uv.x);
    return mix(top, bot, uv.y);
}"#
    )
}

/// Sinusoidal grid warp.
pub fn stock_mesh_warp() -> UvFilter {
    uv_filter!(
        "stock_mesh_warp",
        r#"fn fx_uv_stock_mesh_warp(uv: vec2<f32>, warp: f32, ripple: f32) -> vec2<f32> {
    let jx = (fract(sin(dot(floor(uv * 4.0), vec2<f32>(12.9898, 78.233))) * 43758.5453) - 0.5) * warp * 0.02;
    let jy = (fract(sin(dot(floor(uv * 4.0), vec2<f32>(39.346, 11.135))) * 24634.6345) - 0.5) * warp * 0.02;
    let rx = sin(uv.x * 6.2831853 * 2.5) * cos(uv.y * 6.2831853 * 2.5) * ripple * 0.02;
    let ry = sin(uv.y * 6.2831853 * 2.5) * cos(uv.x * 6.2831853 * 2.5) * ripple * 0.02;
    return uv - vec2<f32>(jx + rx, jy + ry);
}"#
    )
}

/// Zoom-about-center reframe with offset.
pub fn stock_reframe() -> UvFilter {
    uv_filter!(
        "stock_reframe",
        r#"fn fx_uv_stock_reframe(uv: vec2<f32>, scale: f32, offset: vec2<f32>) -> vec2<f32> {
    let c = vec2<f32>(0.5) + offset / 100.0 * 0.5;
    return c + (uv - 0.5) / max(scale / 100.0, 0.1);
}"#
    )
}

/// Parametric liquify push + twirl about a center.
pub fn stock_liquify() -> UvFilter {
    uv_filter!(
        "stock_liquify",
        r#"fn fx_uv_stock_liquify(uv: vec2<f32>, center: vec2<f32>, radius: f32, strength: f32, twirl: f32) -> vec2<f32> {
    let c = vec2<f32>(0.5) + center / 100.0 * 0.5;
    let p = uv - c;
    let r = length(p);
    let rmax = clamp(radius / 100.0, 0.02, 1.0) * 0.5;
    if (r > rmax || r < 0.00001) {
        return uv;
    }
    let fall = 1.0 - r / rmax;
    let push = strength / 100.0 * fall * fall * 0.2;
    let th = atan(p.y, p.x) + radians(twirl) * fall;
    let nr = max(r - push, 0.0);
    return c + vec2<f32>(cos(th), sin(th)) * nr;
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
        levels(),
        hue_saturation(),
        vignette(),
        stock_curves(),
        stock_color_balance(),
        stock_color_wheels(),
        stock_temperature_tint(),
        stock_posterize(),
        stock_threshold(),
        stock_difference_key(),
        stock_spill_suppress(),
        stock_light_leak(),
        stock_scanlines(),
        stock_film_grain(),
        stock_fractal_noise(),
        stock_dust(),
        stock_scratches(),
        stock_flicker(),
        stock_halftone(),
        stock_solid(),
        stock_fractal_gen(),
        stock_grid(),
        stock_shapes(),
        stock_plasma(),
        stock_particles(),
        stock_crop(),
        stock_mosaic(),
        drop_shadow_core(),
    ]
}

/// All uv filters in stable order.
pub fn all_uv_filters() -> Vec<UvFilter> {
    vec![
        perspective(),
        tiler(),
        warp(),
        stock_mirror(),
        stock_repeat(),
        stock_offset(),
        stock_wave(),
        stock_ripple(),
        stock_twirl(),
        stock_bulge(),
        stock_spherize(),
        stock_lens_distortion(),
        stock_pixelate(),
        stock_corner_pin(),
        stock_mesh_warp(),
        stock_reframe(),
        stock_liquify(),
    ]
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
            "levels",
            "hue_saturation",
            "vignette",
            "stock_curves",
            "stock_color_balance",
            "stock_color_wheels",
            "stock_temperature_tint",
            "stock_posterize",
            "stock_threshold",
            "stock_difference_key",
            "stock_spill_suppress",
            "stock_light_leak",
            "stock_scanlines",
            "stock_film_grain",
            "stock_fractal_noise",
            "stock_dust",
            "stock_scratches",
            "stock_flicker",
            "stock_halftone",
            "stock_solid",
            "stock_fractal_gen",
            "stock_grid",
            "stock_shapes",
            "stock_plasma",
            "stock_particles",
            "stock_crop",
            "stock_mosaic",
            "drop_shadow_core",
        ] {
            assert!(ids.contains(&want), "missing {want}");
        }
        let uids: Vec<&str> = all_uv_filters().iter().map(|f| f.id).collect();
        for want in [
            "perspective",
            "tiler",
            "warp",
            "stock_mirror",
            "stock_repeat",
            "stock_offset",
            "stock_wave",
            "stock_ripple",
            "stock_twirl",
            "stock_bulge",
            "stock_spherize",
            "stock_lens_distortion",
            "stock_pixelate",
            "stock_corner_pin",
            "stock_mesh_warp",
            "stock_reframe",
            "stock_liquify",
        ] {
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
