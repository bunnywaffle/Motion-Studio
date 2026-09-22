use compositor::EvaluatedEffectType;
use project::Color;
use super::buffer::FloatBuf;
use super::pixel::Px;

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
        EvaluatedEffectType::GradientRamp { color_a, color_b, angle } => {
            let rad = angle.to_radians();
            let (dx, dy) = (rad.cos(), rad.sin());
            // Project corners to normalize t over the box.
            let corners = [(0.0f32, 0.0f32), (base_w, 0.0), (0.0, base_h), (base_w, base_h)];
            let mut mn = f32::INFINITY;
            let mut mx = f32::NEG_INFINITY;
            for (cx, cy) in corners {
                let t = cx * dx + cy * dy;
                mn = mn.min(t);
                mx = mx.max(t);
            }
            let span = (mx - mn).max(1e-3);
            for y in 0..buf.h {
                for x in 0..buf.w {
                    let dst = buf.get(x as i32, y as i32);
                    if dst.a <= 0.0 {
                        continue;
                    }
                    let lx = x as f32 / w * base_w;
                    let ly = y as f32 / h * base_h;
                    let t = ((lx * dx + ly * dy) - mn) / span;
                    buf.put(
                        x as i32,
                        y as i32,
                        Px {
                            r: (color_a.r + (color_b.r - color_a.r) * t) * dst.a,
                            g: (color_a.g + (color_b.g - color_a.g) * t) * dst.a,
                            b: (color_a.b + (color_b.b - color_a.b) * t) * dst.a,
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
        EvaluatedEffectType::NoiseGenerator { .. }
        | EvaluatedEffectType::GlslShader { .. }
        | EvaluatedEffectType::DisplacementMap { .. }
        | EvaluatedEffectType::GaussianBlur { .. }
        | EvaluatedEffectType::BrightnessContrast { .. }
        | EvaluatedEffectType::Tint { .. }
        | EvaluatedEffectType::Invert { .. }
        | EvaluatedEffectType::DropShadow { .. }
        | EvaluatedEffectType::ChromaKey { .. }
        | EvaluatedEffectType::LumaKey { .. }
        | EvaluatedEffectType::Bloom { .. }
        | EvaluatedEffectType::Exposure { .. }
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
    }
}

fn fx_process_pixel(fx: &EvaluatedEffectType, c: Color) -> Color {
    fx.process_color(c)
}


// ---------------------------------------------------------------------------
