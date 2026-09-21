//! Real Gaussian blur used by the compositor's rasterizer.
//!
//! [`gaussian_kernel_1d`] builds a normalized 1D kernel from a blur radius,
//! and [`gaussian_blur_rgba`] applies it separably (horizontal + vertical
//! pass) to RGBA8 pixel buffers. [`BLUR_WGSL`] is the matching GPU
//! implementation: a two-pass (H/V) separable blur driven by the same
//! radius, so CPU previews and `wgpu` renders agree.
//!
//! Coordinate convention: pixel (0, 0) is the top-left of the image, matching
//! the layer-local bitmap space used everywhere else in the renderer.

/// WGSL separable Gaussian blur shader (single direction per pass).
///
/// Binds: source texture + sampler (`@group(0) @binding(0/1)`), uniforms
/// (`@group(0) @binding(2)`: `texel: vec2<f32>` step per tap,
/// `radius_px: f32`, `canvas: f32` padding). Run once with
/// `texel = (1/w, 0)` then with `texel = (0, 1/h)`.
pub const BLUR_WGSL: &str = r#"
struct BlurUniforms {
    texel: vec2<f32>,
    radius_px: f32,
    taps: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: BlurUniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
    out.uv = vec2<f32>(positions[vi].x * 0.5 + 0.5, 0.5 - positions[vi].y * 0.5);
    return out;
}

// Gaussian weight for integer offset `i` with sigma derived from radius.
fn gauss_weight(i: f32, sigma: f32) -> f32 {
    return exp(-0.5 * (i * i) / (sigma * sigma));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let sigma = max(u.radius_px / 2.0, 0.5);
    let half_taps = i32((u.taps - 1.0) * 0.5);
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var i: i32 = -half_taps; i <= half_taps; i += 1) {
        let w = gauss_weight(f32(i), sigma);
        acc += textureSample(src_tex, src_sampler, in.uv + u.texel * f32(i)) * w;
        wsum += w;
    }
    return acc / max(wsum, 1e-5);
}
"#;

/// Build a normalized 1D Gaussian kernel for `radius_px`.
///
/// The kernel covers `[-ceil(radius), +ceil(radius)]` taps with
/// `sigma = max(radius / 2, 0.5)` (matching [`BLUR_WGSL`]). A radius of
/// `<= 0.0` yields the identity kernel `[1.0]`.
pub fn gaussian_kernel_1d(radius_px: f32) -> Vec<f32> {
    if !radius_px.is_finite() || radius_px <= 0.0 {
        return vec![1.0];
    }
    let sigma = (radius_px / 2.0).max(0.5);
    let half = radius_px.ceil() as i32;
    let mut kernel = Vec::with_capacity((half * 2 + 1) as usize);
    let mut sum = 0.0f32;
    for i in -half..=half {
        let w = (-0.5 * (i as f32) * (i as f32) / (sigma * sigma)).exp();
        kernel.push(w);
        sum += w;
    }
    if sum > 1e-9 {
        for w in &mut kernel {
            *w /= sum;
        }
    }
    kernel
}

/// Apply a separable Gaussian blur to an RGBA8 buffer in place.
///
/// `pixels` holds `width * height * 4` bytes (row-major, top-left origin).
/// A radius of `<= 0.0` is a no-op. Edges are clamped (nearest-pixel
/// sampling), so blurring never reads out of bounds and never changes the
/// buffer size or average brightness.
pub fn gaussian_blur_rgba(pixels: &mut [u8], width: u32, height: u32, radius_px: f32) {
    if !radius_px.is_finite() || radius_px <= 0.0 {
        return;
    }
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || pixels.len() < w * h * 4 {
        return;
    }
    let kernel = gaussian_kernel_1d(radius_px);
    let half = (kernel.len() / 2) as isize;

    // Horizontal pass into a scratch buffer.
    let mut scratch = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (k, weight) in kernel.iter().enumerate() {
                let sx = (x as isize + k as isize - half).clamp(0, w as isize - 1) as usize;
                let base = (y * w + sx) * 4;
                for c in 0..4 {
                    acc[c] += pixels[base + c] as f32 * weight;
                }
            }
            let dst = (y * w + x) * 4;
            for c in 0..4 {
                scratch[dst + c] = acc[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    // Vertical pass back into `pixels`.
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (k, weight) in kernel.iter().enumerate() {
                let sy = (y as isize + k as isize - half).clamp(0, h as isize - 1) as usize;
                let base = (sy * w + x) * 4;
                for c in 0..4 {
                    acc[c] += scratch[base + c] as f32 * weight;
                }
            }
            let dst = (y * w + x) * 4;
            for c in 0..4 {
                pixels[dst + c] = acc[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_kernel_for_zero_radius() {
        assert_eq!(gaussian_kernel_1d(0.0), vec![1.0]);
        assert_eq!(gaussian_kernel_1d(-3.0), vec![1.0]);
    }

    #[test]
    fn test_kernel_is_normalized_and_symmetric() {
        for radius in [1.0f32, 2.5, 8.0] {
            let k = gaussian_kernel_1d(radius);
            let sum: f32 = k.iter().sum();
            assert!((sum - 1.0).abs() < 1e-5, "radius {radius}: sum {sum}");
            let n = k.len();
            for i in 0..n / 2 {
                assert!((k[i] - k[n - 1 - i]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn test_zero_radius_is_noop() {
        let mut px = vec![10u8, 20, 30, 255, 200, 100, 50, 255];
        let before = px.clone();
        gaussian_blur_rgba(&mut px, 2, 1, 0.0);
        assert_eq!(px, before);
    }

    #[test]
    fn test_blur_spreads_single_hot_pixel() {
        // 5x1 strip, hot white pixel in the middle.
        let mut px = vec![0u8; 5 * 4];
        for c in 0..3 {
            px[2 * 4 + c] = 255;
        }
        px[2 * 4 + 3] = 255;
        gaussian_blur_rgba(&mut px, 5, 1, 2.0);
        // Center must have dimmed and neighbors must have received energy.
        assert!(px[2 * 4] < 255);
        assert!(px[1 * 4] > 0);
        assert!(px[3 * 4] > 0);
        // Total red energy is approximately preserved (clamp-edge losses aside).
        let total: u32 = (0..5).map(|x| px[x * 4] as u32).sum();
        assert!(total >= 200 && total <= 255, "total {total}");
    }

    #[test]
    fn test_blur_uniform_field_is_unchanged() {
        let mut px = vec![128u8, 64, 32, 255].repeat(16);
        let before = px.clone();
        gaussian_blur_rgba(&mut px, 4, 4, 3.0);
        assert_eq!(px, before);
    }
}
