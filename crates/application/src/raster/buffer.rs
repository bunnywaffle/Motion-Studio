use super::pixel::Px;
use project::Color;

// Buffers
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct FloatBuf {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Px>,
}

impl FloatBuf {
    pub fn clear(w: u32, h: u32) -> Self {
        Self { w, h, px: vec![Px::clear(); (w.max(1) * h.max(1)) as usize] }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Px {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return Px::clear();
        }
        self.px[(y as u32 * self.w + x as u32) as usize]
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, p: Px) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        self.px[(y as u32 * self.w + x as u32) as usize] = p;
    }

    /// Bilinear sample in pixel space (straight alpha).
    pub fn sample(&self, x: f32, y: f32) -> Px {
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fx = (x - x0 as f32).clamp(0.0, 1.0);
        let fy = (y - y0 as f32).clamp(0.0, 1.0);
        let a = self.get(x0, y0);
        let b = self.get(x0 + 1, y0);
        let c = self.get(x0, y0 + 1);
        let d = self.get(x0 + 1, y0 + 1);
        let mix = |p: f32, q: f32, r: f32, s: f32| {
            p * (1.0 - fx) * (1.0 - fy) + q * fx * (1.0 - fy) + r * (1.0 - fx) * fy + s * fx * fy
        };
        Px {
            r: mix(a.r, b.r, c.r, d.r),
            g: mix(a.g, b.g, c.g, d.g),
            b: mix(a.b, b.b, c.b, d.b),
            a: mix(a.a, b.a, c.a, d.a),
        }
    }

    pub fn average(&self) -> Color {
        if self.px.is_empty() {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0, 0.0, 0.0);
        for p in &self.px {
            r += p.r as f64;
            g += p.g as f64;
            b += p.b as f64;
            a += p.a as f64;
        }
        let n = self.px.len() as f64;
        let avg_a = (a / n) as f32;
        if avg_a <= 1e-6 {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        // Un-premultiply the mean for a straight average color.
        Color::rgba(
            ((r / n) as f32 / avg_a).clamp(0.0, 1.0),
            ((g / n) as f32 / avg_a).clamp(0.0, 1.0),
            ((b / n) as f32 / avg_a).clamp(0.0, 1.0),
            avg_a.clamp(0.0, 1.0),
        )
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let (r, g, b) = if p.a <= 1e-6 {
                (0.0, 0.0, 0.0)
            } else {
                (p.r / p.a, p.g / p.a, p.b / p.a)
            };
            out.push((r.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((g.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((b.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((p.a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }

    /// Straight-alpha BGRA8 bytes for direct `RenderImage` upload (GPUI
    /// caches BGRA frames; feeding PNGs would re-encode, re-hash, and
    /// re-decode per layer per tick with async pop-in).
    pub fn to_bgra8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let (r, g, b) = if p.a <= 1e-6 {
                (0.0, 0.0, 0.0)
            } else {
                (p.r / p.a, p.g / p.a, p.b / p.a)
            };
            out.push((b.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((g.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((r.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((p.a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }
}

use std::sync::{Mutex, OnceLock};
use compositor::fx::stock_params_resolved;
use compositor::{EvaluatedEffect, EvaluatedEffectType};
use renderer::{GpuContext, GpuEffectEngine};

static GPU_ENGINE: OnceLock<Mutex<Option<GpuEffectEngine>>> = OnceLock::new();

fn get_gpu_engine() -> &'static Mutex<Option<GpuEffectEngine>> {
    GPU_ENGINE.get_or_init(|| {
        let engine = GpuContext::new_headless().ok().and_then(|gpu| GpuEffectEngine::new(gpu).ok());
        // Software-emulated adapters are far slower than the CPU kernel
        // (submit + readback stall dominates every call): only keep real
        // hardware for the blur fast path.
        let engine = engine.filter(|e| e.is_hardware());
        Mutex::new(engine)
    })
}

/// True when a real hardware GPU engine is available for accelerated
/// effect passes (status UI + viewport fast paths). Portable wgpu only:
/// software adapters report false and every caller keeps its CPU path.
pub fn gpu_accelerated() -> bool {
    get_gpu_engine().lock().map(|lock| lock.is_some()).unwrap_or(false)
}

/// Write straight-alpha RGBA8 bytes back into a premultiplied float
/// buffer (exact inverse of [`FloatBuf::to_rgba8`]; shared by the blur
/// and effect-chain GPU round trips).
fn write_straight_rgba8(buf: &mut FloatBuf, rgba: &[u8]) {
    for (i, p) in buf.px.iter_mut().enumerate() {
        let a = rgba[i * 4 + 3] as f32 / 255.0;
        *p = Px {
            r: rgba[i * 4] as f32 / 255.0 * a,
            g: rgba[i * 4 + 1] as f32 / 255.0 * a,
            b: rgba[i * 4 + 2] as f32 / 255.0 * a,
            a,
        };
    }
}

/// Run a layer's whole `apply_layer_fx` chain on the GPU when every
/// enabled effect is in the parity-audited [`renderer::CHAIN_SAFE_STOCK`]
/// / [`renderer::CHAIN_SAFE_BUILTIN`] sets (gentle point/rect twins; blur
/// runs in its own stage via `blur_buffer`, steep-curve and time-seeded
/// kernels stay CPU).
/// Params are descriptor-resolved first so packed uniforms match exactly
/// what the CPU kernels see. Returns false for the transparent CPU
/// fallback (no hardware, unsupported chain, any GPU error).
pub fn gpu_fx_chain(buf: &mut FloatBuf, effects: &[EvaluatedEffect], time_s: f32) -> bool {
    if buf.w == 0 || buf.h == 0 {
        return false;
    }
    let Ok(mut lock) = get_gpu_engine().lock() else {
        return false;
    };
    let Some(engine) = lock.as_mut() else {
        return false;
    };
    if !engine.supports_fx_chain(effects) {
        return false;
    }
    let resolved: Vec<EvaluatedEffect> = effects
        .iter()
        .map(|eff| {
            let mut e = eff.clone();
            if let EvaluatedEffectType::Stock { plugin, params, .. } = &e.effect_type {
                let r = stock_params_resolved(*plugin, params).to_vec();
                if let EvaluatedEffectType::Stock { params: dst, .. } = &mut e.effect_type {
                    *dst = r;
                }
            }
            e
        })
        .collect();
    let rgba = buf.to_rgba8();
    let Ok(out) = engine.process_rgba_frame(&rgba, buf.w, buf.h, &resolved, time_s) else {
        return false;
    };
    write_straight_rgba8(buf, &out);
    true
}

/// Full spatial bloom (bright-pass -> blur -> screen) on the GPU.
/// Hardware-gated like the chain: wide radii (> 8px) stay on the CPU
/// kernel, whose downsample path the compute apron cannot reproduce.
/// Returns false for the transparent CPU fallback.
pub fn bloom_buffer(buf: &mut FloatBuf, intensity: f32, radius_px: f32) -> bool {
    let k = (intensity / 100.0).clamp(0.0, 1.0);
    if k <= 0.0 || buf.w == 0 || buf.h == 0 || radius_px > 8.0 {
        return false;
    }
    let Ok(mut lock) = get_gpu_engine().lock() else {
        return false;
    };
    let Some(engine) = lock.as_mut() else {
        return false;
    };
    let rgba = buf.to_rgba8();
    let Ok(out) = engine.bloom_rgba(&rgba, buf.w, buf.h, intensity, radius_px) else {
        return false;
    };
    write_straight_rgba8(buf, &out);
    true
}

/// Gaussian blur a straight-alpha buffer in place.
/// Uses GPU Compute Shader blur with workgroup shared memory tiles when available,
/// falling back seamlessly to the CPU kernel.
/// Wide radii (> 8px) downsample first (cost drops with the square of the
/// factor; gaussian-soft signals resample cleanly), then blur small.
pub fn blur_buffer(buf: &mut FloatBuf, radius_px: f32) {
    let radius_px = radius_px.clamp(0.0, 2048.0);
    if radius_px < 0.5 || buf.w == 0 || buf.h == 0 {
        return;
    }
    if radius_px > 8.0 && (buf.w > 2 || buf.h > 2) {
        // Downsample factor keeps the effective radius <= 8.
        let k = (radius_px / 8.0).ceil().clamp(2.0, 8.0);
        let sw = ((buf.w as f32 / k).ceil() as u32).clamp(2, buf.w);
        let sh = ((buf.h as f32 / k).ceil() as u32).clamp(2, buf.h);
        if sw < buf.w || sh < buf.h {
            let mut small = FloatBuf::clear(sw, sh);
            for y in 0..sh {
                for x in 0..sw {
                    let sx = (x as f32 + 0.5) / sw as f32 * buf.w as f32 - 0.5;
                    let sy = (y as f32 + 0.5) / sh as f32 * buf.h as f32 - 0.5;
                    small.px[(y * sw + x) as usize] = buf.sample(sx, sy);
                }
            }
            blur_buffer(&mut small, radius_px / k);
            for y in 0..buf.h {
                for x in 0..buf.w {
                    let sx = (x as f32 + 0.5) / buf.w as f32 * sw as f32 - 0.5;
                    let sy = (y as f32 + 0.5) / buf.h as f32 * sh as f32 - 0.5;
                    buf.px[(y * buf.w + x) as usize] = small.sample(sx, sy);
                }
            }
            return;
        }
    }
    // Attempt GPU compute blur (its shader apron covers radii up to 8;
    // wider radii silently clamp there, so they take the CPU kernel which
    // honors the full radius and matches across machines).
    if radius_px <= 8.0 {
        if let Ok(mut lock) = get_gpu_engine().lock() {
            if let Some(engine) = lock.as_mut() {
                let bytes = buf.to_rgba8();
                if let Ok(out_rgba) = engine.blur_rgba(&bytes, buf.w, buf.h, radius_px) {
                    write_straight_rgba8(buf, &out_rgba);
                    return;
                }
            }
        }
    }

    // Fallback to CPU kernel
    let mut bytes = buf.to_rgba8();
    renderer::gaussian_blur_rgba(&mut bytes, buf.w, buf.h, radius_px);
    write_straight_rgba8(buf, &bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_softens_hard_edge() {
        let mut buf = FloatBuf::clear(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                if x >= 16 {
                    buf.px[(y * 32 + x) as usize] = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
                }
            }
        }
        blur_buffer(&mut buf, 4.0);
        // Edge column bleeds both ways.
        assert!(buf.get(15, 16).r > 0.05 && buf.get(15, 16).r < 0.95);
        assert!(buf.get(16, 16).r > 0.05 && buf.get(16, 16).r < 1.0);
        // Zero radius is a no-op (and huge radii stay bounded/fast).
        let mut buf2 = FloatBuf::clear(8, 8);
        blur_buffer(&mut buf2, 0.0);
        blur_buffer(&mut buf2, 500.0);
        assert!(buf2.px.iter().all(|p| p.a == 0.0));
    }

    #[test]
    fn wide_blur_matches_direct_kernel_within_tolerance() {
        // 64px white box on transparent, radius 24: downsampled path must
        // agree with the direct CPU kernel (smooth signal, resample-safe).
        fn boxed() -> FloatBuf {
            let mut buf = FloatBuf::clear(64, 64);
            for y in 16..48 {
                for x in 16..48 {
                    buf.px[(y * 64 + x) as usize] = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
                }
            }
            buf
        }
        let mut fast = boxed();
        blur_buffer(&mut fast, 24.0);
        let mut ref_px = boxed();
        let mut bytes = ref_px.to_rgba8();
        renderer::gaussian_blur_rgba(&mut bytes, 64, 64, 24.0);
        for (i, p) in ref_px.px.iter_mut().enumerate() {
            let a = bytes[i * 4 + 3] as f32 / 255.0;
            *p = Px {
                r: bytes[i * 4] as f32 / 255.0 * a,
                g: bytes[i * 4 + 1] as f32 / 255.0 * a,
                b: bytes[i * 4 + 2] as f32 / 255.0 * a,
                a,
            };
        }
        // Deep interior stays strongly covered, far exterior ~clear
        // (gaussian tails never hit exactly zero).
        assert!(fast.get(32, 32).a > 0.7);
        assert!(fast.get(2, 2).a < 0.05);
        let mut worst = 0.0f32;
        for (a, b) in fast.px.iter().zip(ref_px.px.iter()) {
            worst = worst.max((a.a - b.a).abs());
        }
        assert!(worst < 0.12, "downsampled blur drift {worst}");
    }
}
