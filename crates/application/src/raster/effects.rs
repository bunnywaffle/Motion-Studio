use compositor::EvaluatedEffectType;
use project::Color;
use super::buffer::FloatBuf;
use super::pixel::{gradient_axis, gradient_t, Px};
use super::stock::apply_stock;

// Effect application on pixmaps
// ---------------------------------------------------------------------------

/// Frame context for raster effects.
pub struct RasterFx {
    pub time_s: f32,
    pub frame: i64,
    pub res_w: f32,
    pub res_h: f32,
    pub duration_s: f32,
    /// Full per-pixel Shader Lab when false (paused); probe wash when true.
    pub playing: bool,
}

/// Apply one evaluated effect to every pixel of `buf` (local coords).
/// `base_w/h` are the unscaled content dims for uv mapping.
pub fn apply_effect_pixels(
    buf: &mut FloatBuf,
    base_w: f32,
    base_h: f32,
    fx: &EvaluatedEffectType,
    ctx: &RasterFx,
) {
    let w = buf.w as f32;
    let h = buf.h as f32;
    match fx {
        EvaluatedEffectType::Checkerboard { size, color_a, color_b } => {
            let s = (*size).max(2.0);
            for y in 0..buf.h {
                for x in 0..buf.w {
                    // Local-px cells (scale-aware via base mapping below).
                    let lx = x as f32 / w * base_w;
                    let ly = y as f32 / h * base_h;
                    let cell = ((lx / s).floor() + (ly / s).floor()) as i32;
                    let c = if cell & 1 == 0 { *color_a } else { *color_b };
                    let dst = buf.get(x as i32, y as i32);
                    if dst.a > 0.0 {
                        buf.put(
                            x as i32,
                            y as i32,
                            Px { r: c.r * dst.a, g: c.g * dst.a, b: c.b * dst.a, a: dst.a },
                        );
                    }
                }
            }
        }
        EvaluatedEffectType::GradientRamp { gradient } => {
            let axis = gradient_axis(base_w, base_h, gradient.angle);
            for y in 0..buf.h {
                for x in 0..buf.w {
                    let dst = buf.get(x as i32, y as i32);
                    if dst.a <= 0.0 {
                        continue;
                    }
                    let lx = x as f32 / w * base_w;
                    let ly = y as f32 / h * base_h;
                    let c = gradient.sample(gradient_t(lx, ly, axis));
                    buf.put(
                        x as i32,
                        y as i32,
                        Px {
                            r: c.r * dst.a,
                            g: c.g * dst.a,
                            b: c.b * dst.a,
                            a: dst.a,
                        },
                    );
                }
            }
        }
        EvaluatedEffectType::Tiler { tiles_x, tiles_y } => {
            let tx = (*tiles_x).clamp(1.0, 32.0).round().max(1.0);
            let ty = (*tiles_y).clamp(1.0, 32.0).round().max(1.0);
            if tx <= 1.0 && ty <= 1.0 {
                return;
            }
            let src = buf.px.clone();
            let sw = buf.w;
            for y in 0..buf.h {
                for x in 0..buf.w {
                    let sx = ((x as f32 * tx / buf.w as f32).fract() * buf.w as f32) as i32;
                    let sy = ((y as f32 * ty / buf.h as f32).fract() * buf.h as f32) as i32;
                    buf.px[(y * sw + x) as usize] = src[(sy as u32 * sw + sx as u32) as usize];
                }
            }
        }
        EvaluatedEffectType::Warp { amount, scale } => {
            let amp = (*amount / 100.0 * base_w.min(base_h) * 0.25).clamp(0.0, 200.0);
            if amp < 0.25 {
                return;
            }
            let freq = (*scale).clamp(0.1, 10.0) * 0.05;
            let src = buf.px.clone();
            for y in 0..buf.h {
                for x in 0..buf.w {
                    let ox = ((y as f32 * freq).sin() * amp) as i32;
                    let oy = ((x as f32 * freq * 1.3 + 1.7).sin() * amp) as i32;
                    let sx = (x as i32 + ox).clamp(0, buf.w as i32 - 1);
                    let sy = (y as i32 + oy).clamp(0, buf.h as i32 - 1);
                    buf.px[(y * buf.w + x) as usize] =
                        src[(sy as u32 * buf.w + sx as u32) as usize];
                }
            }
        }
        EvaluatedEffectType::Perspective { .. } => {
            // Perspective skew folds into the blit map (affine).
        }
        EvaluatedEffectType::DisplacementMap { max_horizontal, max_vertical } => {
            apply_displacement(buf, *max_horizontal, *max_vertical);
        }
        EvaluatedEffectType::NoiseGenerator { amount, monochrome } => {
            // Time-seeded per frame so grain crawls during playback
            // (the `process_color` twin is the static fallback).
            apply_animated_noise(buf, *amount, *monochrome, ctx.frame);
        }
        EvaluatedEffectType::Sharpen { amount, radius } => {
            apply_sharpen(buf, *amount, *radius);
        }
        EvaluatedEffectType::Vignette { amount, softness } => {
            apply_vignette(buf, *amount, *softness);
        }
        EvaluatedEffectType::GlslShader { .. }
        | EvaluatedEffectType::GaussianBlur { .. }
        | EvaluatedEffectType::BrightnessContrast { .. }
        | EvaluatedEffectType::Tint { .. }
        | EvaluatedEffectType::Invert { .. }
        | EvaluatedEffectType::DropShadow { .. }
        | EvaluatedEffectType::ChromaKey { .. }
        | EvaluatedEffectType::LumaKey { .. }
        | EvaluatedEffectType::Bloom { .. }
        | EvaluatedEffectType::Exposure { .. }
        | EvaluatedEffectType::Levels { .. }
        | EvaluatedEffectType::HueSaturation { .. }
        | EvaluatedEffectType::Vibrance { .. } => {
            // Per-pixel color math shared with the CPU pipeline
            // (premultiplied out, so keying alpha applies).
            for p in buf.px.iter_mut() {
                if p.a <= 0.0 {
                    continue;
                }
                let c = fx_process_pixel(fx, p.to_color());
                *p = Px::from_color(c);
            }
        }
        EvaluatedEffectType::ShaderLab { values, prog, .. } => {
            match prog {
                Some(p) if !ctx.playing => {
                    use project::shader_interp::{PreviewEnv, eval_prog};
                    let env = PreviewEnv {
                        values: values.clone(),
                        time: ctx.time_s,
                        frame: ctx.frame as f32,
                        duration: ctx.duration_s,
                        resolution: (ctx.res_w, ctx.res_h),
                    };
                    for y in 0..buf.h {
                        for x in 0..buf.w {
                            let idx = (y * buf.w + x) as usize;
                            if buf.px[idx].a <= 0.0 {
                                continue;
                            }
                            let uv = (
                                (x as f32 + 0.5) / buf.w as f32,
                                (y as f32 + 0.5) / buf.h as f32,
                            );
                            let c = buf.px[idx].to_color();
                            if let Ok([r, g, b, a]) = eval_prog(p, &env, uv, c) {
                                buf.px[idx] = Px {
                                    r: r.clamp(0.0, 1.0),
                                    g: g.clamp(0.0, 1.0),
                                    b: b.clamp(0.0, 1.0),
                                    a: (a * c.a).clamp(0.0, 1.0),
                                };
                            }
                        }
                    }
                }
                Some(p) => {
                    // Playback: cheap probe wash in the grade direction.
                    use project::shader_interp::{PreviewEnv, eval_prog};
                    let env = PreviewEnv {
                        values: values.clone(),
                        time: ctx.time_s,
                        frame: ctx.frame as f32,
                        duration: ctx.duration_s,
                        resolution: (ctx.res_w, ctx.res_h),
                    };
                    let probe = Color::rgba(0.5, 0.5, 0.5, 1.0);
                    if let Ok([r, g, b, _]) = eval_prog(p, &env, (0.5, 0.5), probe) {
                        let lift = Px {
                            r: (r - 0.5) * 0.5,
                            g: (g - 0.5) * 0.5,
                            b: (b - 0.5) * 0.5,
                            a: 0.0,
                        };
                        if lift.r.abs() + lift.g.abs() + lift.b.abs() > 0.02 {
                            for p in buf.px.iter_mut() {
                                if p.a > 0.0 {
                                    p.r = (p.r + lift.r).clamp(0.0, 1.0);
                                    p.g = (p.g + lift.g).clamp(0.0, 1.0);
                                    p.b = (p.b + lift.b).clamp(0.0, 1.0);
                                }
                            }
                        }
                    }
                }
                None => {}
            }
        }
        EvaluatedEffectType::TextOutline { .. } | EvaluatedEffectType::TextBevel { .. } => {
            // Resolved inside the text rasterizer.
        }
        EvaluatedEffectType::Stock { plugin, params, colors } => {
            apply_stock(buf, *plugin, params, colors, ctx);
        }
    }
}

fn fx_process_pixel(fx: &EvaluatedEffectType, c: Color) -> Color {
    fx.process_color(c)
}

/// Film-style animated grain in place, seeded per frame so it crawls
/// during playback instead of sitting static.
pub fn apply_animated_noise(buf: &mut FloatBuf, amount: f32, monochrome: bool, frame: i64) {
    let k = (amount / 100.0).clamp(0.0, 1.0);
    if k <= 0.001 {
        return;
    }
    let seed = (frame as f32 + 1.0) * 0.6180339;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let p = &mut buf.px[idx];
            if p.a <= 0.0 {
                continue;
            }
            // Deterministic per-pixel, per-frame hash in [0, 1).
            let h1 = ((x as f32 * 12.9898 + y as f32 * 78.233 + seed * 45.164).sin() * 43_758.547).fract();
            if monochrome {
                let n = (h1 - 0.5) * k;
                p.r = (p.r + n).clamp(0.0, 1.0);
                p.g = (p.g + n).clamp(0.0, 1.0);
                p.b = (p.b + n).clamp(0.0, 1.0);
            } else {
                let h2 = ((x as f32 * 39.346 + y as f32 * 11.135 + seed * 93.422).sin() * 24_634.635).fract();
                let h3 = ((x as f32 * 73.156 + y as f32 * 5.317 + seed * 17.123).sin() * 56_445.234).fract();
                p.r = (p.r + (h1 - 0.5) * k).clamp(0.0, 1.0);
                p.g = (p.g + (h2 - 0.5) * k).clamp(0.0, 1.0);
                p.b = (p.b + (h3 - 0.5) * k).clamp(0.0, 1.0);
            }
        }
    }
}

/// Luminance-driven displacement in place: each pixel is resampled from
/// `(x - (luma - 0.5) * max_h, y - (luma - 0.5) * max_v)` with bilinear
/// filtering, so bright areas push one way and dark areas the other
/// (self-map mode; a dedicated map layer is a future input).
pub fn apply_displacement(buf: &mut FloatBuf, max_horizontal: f32, max_vertical: f32) {
    if max_horizontal.abs() < 0.05 && max_vertical.abs() < 0.05 {
        return;
    }
    let src = buf.px.clone();
    let snap = FloatBuf { w: buf.w, h: buf.h, px: src.clone() };
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let p = src[idx];
            if p.a <= 0.0 {
                continue;
            }
            let ia = 1.0 / p.a.max(1e-6);
            let lum = (0.299 * p.r + 0.587 * p.g + 0.114 * p.b) * ia;
            let ox = (lum - 0.5) * max_horizontal;
            let oy = (lum - 0.5) * max_vertical;
            buf.px[idx] = snap.sample(x as f32 - ox, y as f32 - oy);
        }
    }
}

/// Unsharp-mask sharpen in place: `out = orig + amount * (orig - blurred)`.
/// `amount` is 0..200 (% of the high-frequency detail added back),
/// `radius` the blur sigma in buffer px (capped by `blur_buffer`).
pub fn apply_sharpen(buf: &mut FloatBuf, amount: f32, radius: f32) {
    use super::buffer::blur_buffer;
    let k = (amount / 100.0).clamp(0.0, 2.0);
    if k <= 0.01 {
        return;
    }
    let mut blurred = FloatBuf { w: buf.w, h: buf.h, px: buf.px.clone() };
    blur_buffer(&mut blurred, radius.max(0.5));
    for (dst, avg) in buf.px.iter_mut().zip(blurred.px.iter()) {
        if dst.a <= 0.0 {
            continue;
        }
        // Work on straight (un-premultiplied) color so dark fringes stay clean.
        let ia = 1.0 / dst.a.max(1e-6);
        let ib = 1.0 / avg.a.max(1e-6);
        let sharpen = |x: f32, y: f32| (x + (x - y) * k).clamp(0.0, 1.0) * dst.a;
        dst.r = sharpen(dst.r * ia, avg.r * ib);
        dst.g = sharpen(dst.g * ia, avg.g * ib);
        dst.b = sharpen(dst.b * ia, avg.b * ib);
    }
}

/// Edge vignette in place: darkens toward the frame corners.
/// `amount` 0..100 scales the falloff, `softness` 0..100 widens the
/// transition band (100 = feathered to the center).
pub fn apply_vignette(buf: &mut FloatBuf, amount: f32, softness: f32) {
    let k = (amount / 100.0).clamp(0.0, 1.0);
    if k <= 0.001 || buf.w == 0 || buf.h == 0 {
        return;
    }
    let soft = (softness / 100.0).clamp(0.0, 1.0);
    // Inner radius shrinks as softness grows: hard edge vs long feather.
    let inner = 0.5 * (1.0 - soft * 0.85);
    let outer = 0.5 + 0.28 * (1.0 - soft * 0.4);
    let span = (outer - inner).max(1e-3);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            if buf.px[idx].a <= 0.0 {
                continue;
            }
            let nx = (x as f32 + 0.5) / buf.w as f32 * 2.0 - 1.0;
            let ny = (y as f32 + 0.5) / buf.h as f32 * 2.0 - 1.0;
            // Elliptical distance so wide frames fall off evenly.
            let d = (nx * nx + ny * ny).sqrt() / std::f32::consts::SQRT_2;
            let t = ((d - inner) / span).clamp(0.0, 1.0);
            // Smoothstep the band to avoid ringing.
            let s = t * t * (3.0 - 2.0 * t);
            let m = 1.0 - k * s;
            let p = &mut buf.px[idx];
            p.r *= m;
            p.g *= m;
            p.b *= m;
        }
    }
}


// ---------------------------------------------------------------------------
