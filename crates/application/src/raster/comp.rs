use compositor::EvaluatedEffectType;
use image::RgbaImage;
use project::{Color, LayerSource};
use std::collections::HashMap;
use std::sync::Arc;
use super::affine::{Aff, aff_mul, fold_transform, skew_about};
use super::buffer::{FloatBuf, blur_buffer};
use super::effects::RasterFx;use super::layer::{apply_adjustment, apply_bloom, apply_layer_fx, blit_affine, layer_base_dims, raster_layer_content};
use super::pixel::Px;

/// Full composition raster at `out_w` x `out_h` (canvas px).
pub fn rasterize_comp(
    stack: &compositor::EvaluatedStack,
    comp_w: f32,
    comp_h: f32,
    bg: Color,
    out_w: u32,
    out_h: u32,
    time_s: f32,
    frame: i64,
    playing: bool,
    duration_s: f32,
    assets: &mut HashMap<String, Arc<RgbaImage>>,
) -> FloatBuf {
    let (ow, oh) = (out_w.max(1), out_h.max(1));
    let mut dst = FloatBuf::clear(ow, oh);
    // Background (transparent comps get a checkerboard).
    if bg.a >= 0.999 {
        let p = Px::from_color(bg);
        for px in dst.px.iter_mut() {
            *px = p;
        }
    } else {
        let cell = 12.0;
        for y in 0..oh {
            for x in 0..ow {
                let on = ((x as f32 / cell).floor() + (y as f32 / cell).floor()) as i32 & 1 == 0;
                let g = if on { 0.16 } else { 0.11 };
                dst.px[(y * ow + x) as usize] = Px { r: g, g, b: g, a: 1.0 };
            }
        }
        if bg.a > 0.0 {
            let p = Px::from_color(bg);
            for px in dst.px.iter_mut() {
                let mut out = *px;
                out.over(p);
                *px = out;
            }
        }
    }

    let fx = RasterFx {
        time_s,
        frame,
        res_w: comp_w,
        res_h: comp_h,
        duration_s,
        playing,
    };
    // Scale from composition px to output px.
    let k = ow as f32 / comp_w.max(1.0);

    for layer in stack.render_layers() {
        if !layer.is_rendered() {
            continue;
        }
        if matches!(&layer.source, LayerSource::Adjustment) {
            // True AE semantics: adjustment layers post-process everything
            // composited beneath them.
            apply_adjustment(&mut dst, &layer.effects, &fx);
            continue;
        }
        // Base dims mirror the viewer estimate (anchor/pivot consistent).
        let (base_w, base_h) = layer_base_dims(layer, comp_w, comp_h, assets);
        // Local->world affine from the evaluated matrix, scaled to output.
        let wm = layer.world_matrix();
        let mut map = Aff {
            a: wm.a * k,
            b: wm.b * k,
            c: wm.c * k,
            d: wm.d * k,
            tx: (wm.tx + comp_w / 2.0) * k,
            ty: (wm.ty + comp_h / 2.0) * k,
        };
        // Perspective skew folds into the map (local skew about center).
        for eff in &layer.effects {
            if !eff.enabled {
                continue;
            }
            if let EvaluatedEffectType::Perspective { skew_x, skew_y } = &eff.effect_type {
                if skew_x.abs() >= 0.05 || skew_y.abs() >= 0.05 {
                    map = aff_mul(map, skew_about(*skew_x, *skew_y, base_w / 2.0, base_h / 2.0));
                }
                break;
            }
        }
        // Transform stock plug-in folds into the map (output-space).
        for eff in &layer.effects {
            if !eff.enabled {
                continue;
            }
            if let EvaluatedEffectType::Stock { plugin, params, .. } = &eff.effect_type {
                if *plugin == project::StockPlugin::TransformFx {
                    use compositor::fx::stock_p;
                    map = fold_transform(
                        map,
                        ow as f32 * 0.5,
                        oh as f32 * 0.5,
                        stock_p(*plugin, params, 0) * k,
                        stock_p(*plugin, params, 1) * k,
                        stock_p(*plugin, params, 2),
                        stock_p(*plugin, params, 3),
                    );
                }
            }
        }
        // Output bounds: world AABB in output px.
        let bbox = layer.world_bounds(base_w, base_h);
        let x0 = ((bbox.min.x + comp_w / 2.0) * k).floor().max(0.0) as u32;
        let y0 = ((bbox.min.y + comp_h / 2.0) * k).floor().max(0.0) as u32;
        let x1 = ((bbox.max.x + comp_w / 2.0) * k).ceil().min(ow as f32) as u32;
        let y1 = ((bbox.max.y + comp_h / 2.0) * k).ceil().min(oh as f32) as u32;
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        // Local content pixmap (comp px; images native).
        let content = raster_layer_content(layer, base_w, base_h, assets);
        let Some(content) = content else { continue };
        // Per-pixel local effects (checker/gradient/tiler/warp/noise/
        // shader/colors...).
        {
            let mut work = content;
            apply_layer_fx(&mut work, base_w, base_h, &layer.effects, &fx);
            // Blur + bloom radius (convolution on the content pixmap).
            let mut blur_total = 0.0f32;
            let mut bloom: Option<(f32, f32)> = None;
            for eff in &layer.effects {
                if !eff.enabled {
                    continue;
                }
                match &eff.effect_type {
                    EvaluatedEffectType::GaussianBlur { radius } => blur_total += *radius,
                    EvaluatedEffectType::Bloom { intensity, radius } => {
                        if *intensity > 0.5 {
                            bloom = Some((*intensity, *radius));
                        }
                    }
                    _ => {}
                }
            }
            if blur_total > 0.25 {
                blur_buffer(&mut work, blur_total * k.max(0.5));
            }
            if let Some((intensity, radius)) = bloom {
                apply_bloom(&mut work, intensity, radius * k.max(0.5));
            }
            // Drop shadow params (drawn under at blit).
            let mut shadow = None;
            for eff in &layer.effects {
                if !eff.enabled {
                    continue;
                }
                if let EvaluatedEffectType::DropShadow { distance, angle, opacity, color, .. } =
                    &eff.effect_type
                {
                    let rad = angle.to_radians();
                    shadow = Some((
                        distance * rad.cos() * k,
                        distance * rad.sin() * k,
                        (opacity / 100.0).clamp(0.0, 1.0) * 0.75,
                        *color,
                    ));
                    break;
                }
            }
            // Blit sub-region.
            let mut sub = FloatBuf::clear(x1 - x0, y1 - y0);
            // NOTE: blit_affine walks dst pixels; map translates output px
            // to local px, so shift the map by the sub-region origin.
            let shifted = Aff {
                a: map.a,
                b: map.b,
                c: map.c,
                d: map.d,
                tx: map.tx - x0 as f32,
                ty: map.ty - y0 as f32,
            };
            // Shadow needs the content blurred for softness: reuse blur.
            blit_affine(
                &mut sub,
                &work,
                shifted,
                layer.effective_opacity.clamp(0.0, 1.0),
                layer.blend_mode,
                shadow,
                None,
            );
            // Composite sub-region back (already blended vs transparent;
            // blend vs backdrop per pixel using sampled backdrop average is
            // handled by blending vs the live buffer for Normal; for exotic
            // modes blend vs the actual buffer pixels below).
            for y in 0..sub.h {
                for x in 0..sub.w {
                    let s = sub.px[(y * sub.w + x) as usize];
                    if s.a <= 0.003 {
                        continue;
                    }
                    let dx = x0 + x;
                    let dy = y0 + y;
                    let mut d = dst.px[(dy * ow + dx) as usize];
                    d.blend_over(s, layer.blend_mode);
                    dst.px[(dy * ow + dx) as usize] = d;
                }
            }
        }
    }
    dst
}
