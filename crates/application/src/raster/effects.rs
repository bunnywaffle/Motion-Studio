use compositor::EvaluatedEffectType;
use project::Color;
use super::buffer::FloatBuf;
use super::pixel::{gradient_axis, sample_fill_gradient, Px};
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
                    let c = sample_fill_gradient(gradient, lx, ly, axis);
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
        EvaluatedEffectType::Tiler {
            tiles_x,
            tiles_y,
            mode,
            mirror,
            offset_x,
            offset_y,
            cell,
            seed,
            amount,
        } => {
            let tx = (*tiles_x).clamp(1.0, 64.0).floor().max(1.0);
            let ty = (*tiles_y).clamp(1.0, 64.0).floor().max(1.0);
            let off_x = *offset_x;
            let off_y = *offset_y;
            let amt = (*amount / 100.0).clamp(0.0, 1.0);
            let src = buf.px.clone();
            let sw = buf.w;
            let sh = buf.h;
            let w_f = buf.w.max(1) as f32;
            let h_f = buf.h.max(1) as f32;

            for y in 0..buf.h {
                for x in 0..buf.w {
                    let u = x as f32 / w_f;
                    let v = y as f32 / h_f;

                    let (cell_id_x, cell_id_y, mut local_u, mut local_v) = match mode {
                        project::TileMode::Radial => {
                            let cx = u - 0.5;
                            let cy = v - 0.5;
                            let radius = (cx * cx + cy * cy).sqrt() * 2.0;
                            let phi = cy.atan2(cx);
                            let ang = phi / (2.0 * std::f32::consts::PI) + 0.5;
                            let px = ang * tx + off_x;
                            let py = radius * ty + off_y;
                            (px.floor(), py.floor(), px.fract(), py.fract())
                        }
                        project::TileMode::Hex => {
                            let r3 = 1.7320508_f32;
                            let px = (u + off_x) * tx;
                            let py = (v + off_y) * ty * r3;
                            let rx = 1.0_f32;
                            let ry = r3;
                            let hx = rx * 0.5;
                            let hy = ry * 0.5;
                            let ax = px - rx * (px / rx).floor();
                            let ay = py - ry * (py / ry).floor();
                            let bx = (px - hx) - rx * ((px - hx) / rx).floor();
                            let by = (py - hy) - ry * ((py - hy) / ry).floor();
                            let pa_x = ax - hx;
                            let pa_y = ay - hy;
                            let pb_x = bx - hx;
                            let pb_y = by - hy;
                            if pa_x * pa_x + pa_y * pa_y < pb_x * pb_x + pb_y * pb_y {
                                ((px / rx).floor(), (py / ry).floor(), pa_x / rx + 0.5, pa_y / ry + 0.5)
                            } else {
                                (((px - hx) / rx).floor() + 0.5, ((py - hy) / ry).floor() + 0.5, pb_x / rx + 0.5, pb_y / ry + 0.5)
                            }
                        }
                        project::TileMode::Triangle => {
                            let px = u * tx + off_x;
                            let py = v * ty + off_y;
                            let qx = px.floor();
                            let qy = py.floor();
                            let mut fx = px.fract();
                            let mut fy = py.fract();
                            let mut tid_x = qx * 2.0;
                            if fx + fy > 1.0 {
                                tid_x += 1.0;
                                fx = 1.0 - fx;
                                fy = 1.0 - fy;
                            }
                            (tid_x, qy, fx, fy)
                        }
                        project::TileMode::Grid => {
                            let px = u * tx + off_x;
                            let py = v * ty + off_y;
                            (px.floor(), py.floor(), px.fract(), py.fract())
                        }
                    };

                    if *mirror {
                        let ix = (cell_id_x.abs() as i64) % 2;
                        let iy = (cell_id_y.abs() as i64) % 2;
                        if ix == 1 {
                            local_u = 1.0 - local_u;
                        }
                        if iy == 1 {
                            local_v = 1.0 - local_v;
                        }
                    }

                    let cell_u = local_u;
                    let cell_v = local_v;
                    let mut sample_u = local_u;
                    let mut sample_v = local_v;

                    if amt > 0.0 {
                        let hash22 = |px: f32, py: f32, s: f32| -> (f32, f32) {
                            let f = |val: f32| val - val.floor();
                            let dot = |ax: f32, ay: f32, az: f32, bx: f32, by: f32, bz: f32| ax * bx + ay * by + az * bz;
                            let p3_x = f(px * 443.897 + s * 19.19);
                            let p3_y = f(py * 441.423 + s * 7.31);
                            let p3_z = f(px * 437.195 + s * 13.73);
                            let d = dot(p3_x, p3_y, p3_z, p3_y + 19.19, p3_z + 19.19, p3_x + 19.19);
                            (f((p3_x + p3_y) * d), f((p3_x + p3_z) * d))
                        };

                        let (rnd1_x, rnd1_y) = hash22(cell_id_x, cell_id_y, *seed);
                        let (rnd2_x, rnd2_y) = hash22(cell_id_y + 13.37, cell_id_x + 73.31, *seed + 31.0);

                        let mut qx = local_u - 0.5;
                        let mut qy = local_v - 0.5;
                        let rot = (rnd1_x - 0.5) * std::f32::consts::TAU * amt;
                        let cos_r = rot.cos();
                        let sin_r = rot.sin();
                        let n_qx = qx * cos_r - qy * sin_r;
                        let n_qy = qx * sin_r + qy * cos_r;
                        qx = n_qx;
                        qy = n_qy;

                        if amt >= 0.5 && rnd1_y > 0.5 {
                            qx = -qx;
                        }

                        let jit_x = (rnd2_x - 0.5) * amt * 0.5;
                        let jit_y = (rnd2_y - 0.5) * amt * 0.5;
                        sample_u = (qx + 0.5 + jit_x).fract();
                        sample_v = (qy + 0.5 + jit_y).fract();
                        if sample_u < 0.0 { sample_u += 1.0; }
                        if sample_v < 0.0 { sample_v += 1.0; }
                    }

                    let qx = cell_u - 0.5;
                    let qy = cell_v - 0.5;
                    let inside = match cell {
                        project::TileCell::Square => qx.abs() <= 0.5 && qy.abs() <= 0.5,
                        project::TileCell::Diamond => (qx.abs() + qy.abs()) <= 0.5,
                        project::TileCell::Circle => (qx * qx + qy * qy).sqrt() <= 0.5,
                        project::TileCell::Triangle => qy >= -0.35 && (qx.abs() * 1.7320508 + qy) <= 0.5,
                        project::TileCell::Hexagon => qx.abs() <= 0.45 && (qx.abs() * 0.5 + qy.abs() * 0.8660254) <= 0.45,
                    };

                    let idx = (y * sw + x) as usize;
                    if !inside {
                        buf.px[idx] = Px::clear();
                    } else {
                        let sx = ((sample_u.clamp(0.0, 1.0) * (sw as f32 - 1.0)).round() as u32).min(sw - 1);
                        let sy = ((sample_v.clamp(0.0, 1.0) * (sh as f32 - 1.0)).round() as u32).min(sh - 1);
                        buf.px[idx] = src[(sy * sw + sx) as usize];
                    }
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
        | EvaluatedEffectType::SwapColor { .. }
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
                    let (eval_w, eval_h) = {
                        let max_w = 320u32;
                        let max_h = 180u32;
                        if buf.w <= max_w && buf.h <= max_h {
                            (buf.w, buf.h)
                        } else {
                            let sx = max_w as f32 / buf.w as f32;
                            let sy = max_h as f32 / buf.h as f32;
                            let s = sx.min(sy);
                            ((buf.w as f32 * s).max(1.0) as u32, (buf.h as f32 * s).max(1.0) as u32)
                        }
                    };
                    if eval_w < buf.w || eval_h < buf.h {
                        let bx = buf.w as f32 / eval_w as f32;
                        let by = buf.h as f32 / eval_h as f32;
                        let mut small = FloatBuf::clear(eval_w, eval_h);
                        for ey in 0..eval_h {
                            for ex in 0..eval_w {
                                let sx = (ex as f32 + 0.5) * bx;
                                let sy = (ey as f32 + 0.5) * by;
                                small.px[(ey * eval_w + ex) as usize] = buf.sample(sx - 0.5, sy - 0.5);
                            }
                        }
                        for ey in 0..eval_h {
                            for ex in 0..eval_w {
                                let idx = (ey * eval_w + ex) as usize;
                                if small.px[idx].a <= 0.0 { continue; }
                                let uv = ((ex as f32 + 0.5) / eval_w as f32, (ey as f32 + 0.5) / eval_h as f32);
                                let c = small.px[idx].to_color();
                                if let Ok([r, g, b, a]) = eval_prog(p, &env, uv, c) {
                                    small.px[idx] = Px { r: r.clamp(0.0,1.0), g: g.clamp(0.0,1.0), b: b.clamp(0.0,1.0), a: (a * c.a).clamp(0.0,1.0) };
                                }
                            }
                        }
                        for y in 0..buf.h {
                            for x in 0..buf.w {
                                if buf.px[(y * buf.w + x) as usize].a <= 0.0 { continue; }
                                let sx = (x as f32 + 0.5) / buf.w as f32 * eval_w as f32 - 0.5;
                                let sy = (y as f32 + 0.5) / buf.h as f32 * eval_h as f32 - 0.5;
                                buf.px[(y * buf.w + x) as usize] = small.sample(sx, sy);
                            }
                        }
                    } else {
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
        EvaluatedEffectType::TextOutline { .. }
        | EvaluatedEffectType::TextBevel { .. }
        | EvaluatedEffectType::TextSplitAnimator { .. }
        | EvaluatedEffectType::OuterGlow { .. } => {
            // Resolved inside the text/blit rasterizer.
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

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::evaluation::EvaluatedEffectType;

    #[test]
    fn test_tiler_grid_mirror_and_shapes() {
        let mut buf = FloatBuf {
            w: 100,
            h: 100,
            px: vec![Px { r: 1.0, g: 0.5, b: 0.25, a: 1.0 }; 100 * 100],
        };
        let ctx = RasterFx {
            time_s: 0.0,
            frame: 0,
            res_w: 100.0,
            res_h: 100.0,
            duration_s: 10.0,
            playing: false,
        };

        // Mode: Grid, Shape: Circle, Mirror: true, Random: 50%
        let eff = EvaluatedEffectType::Tiler {
            tiles_x: 2.0,
            tiles_y: 2.0,
            mode: project::TileMode::Grid,
            mirror: true,
            offset_x: 0.0,
            offset_y: 0.0,
            cell: project::TileCell::Circle,
            seed: 12.0,
            amount: 50.0,
        };
        apply_effect_pixels(&mut buf, 100.0, 100.0, &eff, &ctx);

        // Outside circular aperture should be transparent
        // Top-left pixel (0,0) is outside the circle of the top-left tile
        assert_eq!(buf.px[0].a, 0.0);
        // Center of top-left tile (25, 25) is inside
        assert!(buf.px[25 * 100 + 25].a > 0.0);
    }

    #[test]
    fn test_tiler_radial_and_hex_modes() {
        for mode in [project::TileMode::Radial, project::TileMode::Hex, project::TileMode::Triangle] {
            let mut buf = FloatBuf {
                w: 60,
                h: 60,
                px: vec![Px { r: 0.8, g: 0.2, b: 0.4, a: 1.0 }; 60 * 60],
            };
            let ctx = RasterFx {
                time_s: 0.0,
                frame: 0,
                res_w: 60.0,
                res_h: 60.0,
                duration_s: 10.0,
                playing: false,
            };
            let eff = EvaluatedEffectType::Tiler {
                tiles_x: 3.0,
                tiles_y: 3.0,
                mode,
                mirror: false,
                offset_x: 0.1,
                offset_y: 0.1,
                cell: project::TileCell::Square,
                seed: 42.0,
                amount: 30.0,
            };
            apply_effect_pixels(&mut buf, 60.0, 60.0, &eff, &ctx);
            // Verify buffer has pixels with ink
            let ink_count = buf.px.iter().filter(|p| p.a > 0.5).count();
            assert!(ink_count > 0, "mode {mode:?} should render ink");
        }
    }
}
