use compositor::EvaluatedEffectType;
use image::RgbaImage;
use project::{BlendMode, Color, LayerSource};
use std::collections::HashMap;
use std::sync::Arc;
use super::affine::{Aff, aff_mul, fold_transform, skew_about};
use super::buffer::{FloatBuf, blur_buffer};
use super::effects::RasterFx;use super::layer::{apply_adjustment, apply_bloom, apply_layer_fx, blit_affine, layer_base_dims, layer_effect_padding, layer_local_box, layer_render_box, raster_layer_content};
use super::mask::apply_masks;
use super::pixel::Px;

/// Full composition raster at `out_w` x `out_h` (canvas px).
#[allow(clippy::too_many_arguments)]
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
            // composited beneath them, respecting masks, opacity, and blend mode.
            let (base_w, base_h) = layer_base_dims(layer, comp_w, comp_h, assets);
            let pristine = dst.clone();
            let mut work = dst.clone();
            apply_adjustment(&mut work, &layer.effects, &fx);

            let mask_cov = if !layer.masks.is_empty() {
                let sx = ow as f32 / base_w.max(1.0);
                let sy = oh as f32 / base_h.max(1.0);
                let scale_xform = compositor::AffineTransform2D::from_scale(project::Vec2::new(sx, sy));
                let scaled_masks: Vec<compositor::EvaluatedMask> = layer
                    .masks
                    .iter()
                    .map(|m| {
                        let mut sm = m.clone();
                        sm.transform.local_matrix = scale_xform * sm.transform.local_matrix;
                        sm.feather *= (sx + sy) * 0.5;
                        sm.expansion *= (sx + sy) * 0.5;
                        sm
                    })
                    .collect();
                crate::raster::mask::evaluate_mask_coverage(ow, oh, &scaled_masks)
            } else {
                None
            };

            let eff_op = layer.effective_opacity.clamp(0.0, 1.0);
            for y in 0..oh {
                for x in 0..ow {
                    let idx = (y * ow + x) as usize;
                    let orig = pristine.px[idx];
                    let adj = work.px[idx];
                    let m = mask_cov.as_ref().map(|cov| cov[idx]).unwrap_or(1.0);
                    let alpha = (m * eff_op).clamp(0.0, 1.0);

                    if alpha <= 0.001 {
                        continue;
                    }

                    let target_px = if layer.blend_mode == BlendMode::Normal {
                        adj
                    } else {
                        let mut b = orig;
                        b.blend_over_at(adj, layer.blend_mode, x as i32, y as i32);
                        b
                    };

                    dst.px[idx] = Px {
                        r: orig.r + (target_px.r - orig.r) * alpha,
                        g: orig.g + (target_px.g - orig.g) * alpha,
                        b: orig.b + (target_px.b - orig.b) * alpha,
                        a: orig.a + (target_px.a - orig.a) * alpha,
                    };
                }
            }
            continue;
        }
        // Base dims mirror the viewer estimate (anchor/pivot consistent).
        // Pen/path shapes use their raster frame (arbitrary local coords).
        let (base_w, base_h) = layer_base_dims(layer, comp_w, comp_h, assets);
        let local_box = layer_local_box(layer, base_w, base_h);
        let render_box = layer_render_box(layer, base_w, base_h);
        let pad = layer_effect_padding(layer);
        let (frame_ox, frame_oy) = (local_box.min.x, local_box.min.y);
        let (eff_ox, eff_oy) = if pad > 0.0 {
            (frame_ox - pad, frame_oy - pad)
        } else {
            (frame_ox, frame_oy)
        };
        // Local->world affine from the evaluated matrix, scaled to output.
        // Content px (0, 0) is local `frame origin`, hence `W * origin`.
        let wm = layer.world_matrix();
        let mut map = Aff {
            a: wm.a * k,
            b: wm.b * k,
            c: wm.c * k,
            d: wm.d * k,
            tx: (wm.a * eff_ox + wm.c * eff_oy + wm.tx + comp_w / 2.0) * k,
            ty: (wm.b * eff_ox + wm.d * eff_oy + wm.ty + comp_h / 2.0) * k,
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
        // Output bounds: world AABB of the local content box in output px.
        let bbox = layer.local_to_world_bbox(&render_box);
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
            // Masks shape alpha before effects (AE order). Framed (path)
            // layers raster frame-relative, so mask paths shift by `-origin`.
            if frame_ox != 0.0 || frame_oy != 0.0 {
                use compositor::AffineTransform2D;
                use project::Vec2;
                let shift =
                    AffineTransform2D::from_translation(Vec2::new(-frame_ox, -frame_oy));
                let shifted_masks: Vec<compositor::EvaluatedMask> = layer
                    .masks
                    .iter()
                    .map(|m| {
                        let mut m = m.clone();
                        let lm = m.transform.local_matrix;
                        m.transform.local_matrix = shift * lm;
                        m
                    })
                    .collect();
                apply_masks(&mut work, &shifted_masks);
            } else {
                apply_masks(&mut work, &layer.masks);
            }
            if pad > 0.0 {
                let pad_u = pad as u32;
                let mut padded = FloatBuf::clear(work.w + pad_u * 2, work.h + pad_u * 2);
                for y in 0..work.h {
                    for x in 0..work.w {
                        padded.put((x + pad_u) as i32, (y + pad_u) as i32, work.get(x as i32, y as i32));
                    }
                }
                work = padded;
            }
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
                    EvaluatedEffectType::Bloom { intensity, radius }
                        if *intensity > 0.5 => {
                            bloom = Some((*intensity, *radius));
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
            // Outer glow blit
            for eff in &layer.effects {
                if !eff.enabled {
                    continue;
                }
                if let EvaluatedEffectType::OuterGlow { size, spread, opacity, color, .. } =
                    &eff.effect_type
                {
                    let glow_radius = (size * k).clamp(0.5, 2048.0);
                    let glow_opacity = (opacity / 100.0).clamp(0.0, 1.0);
                    if glow_opacity > 0.01 && glow_radius > 0.5 {
                        let mut sil = FloatBuf::clear(work.w, work.h);
                        let spread_val = *spread;
                        for (d, s) in sil.px.iter_mut().zip(work.px.iter()) {
                            let a = if spread_val > 5.0 {
                                (s.a * (1.0 + spread_val / 20.0)).clamp(0.0, 1.0)
                            } else {
                                s.a
                            };
                            *d = Px { r: 0.0, g: 0.0, b: 0.0, a };
                        }
                        crate::raster::buffer::blur_buffer(&mut sil, glow_radius);
                        let mut glow_sub = FloatBuf::clear(x1 - x0, y1 - y0);
                        let glow_shifted = Aff {
                            a: map.a,
                            b: map.b,
                            c: map.c,
                            d: map.d,
                            tx: map.tx - x0 as f32,
                            ty: map.ty - y0 as f32,
                        };
                        crate::raster::layer::blit_affine(
                            &mut glow_sub,
                            &work,
                            glow_shifted,
                            layer.effective_opacity.clamp(0.0, 1.0),
                            BlendMode::Normal,
                            Some((0.0, 0.0, glow_opacity, *color)),
                            Some(&sil),
                        );
                        for y in 0..glow_sub.h {
                            for x in 0..glow_sub.w {
                                let gp = glow_sub.px[(y * glow_sub.w + x) as usize];
                                if gp.a > 0.003 {
                                    let idx = ((y0 + y) * ow + (x0 + x)) as usize;
                                    dst.px[idx].blend_over(gp, BlendMode::Normal);
                                }
                            }
                        }
                    }
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
            // Blit isolated with Normal (transparent backdrop): exotic
            // blend math below runs ONCE against the live backdrop pixels.
            // Blending here too would apply the mode twice (once vs
            // transparent black, which zeroes multiplicative modes) and
            // corrupt every non-Normal layer (notably opaque shape fills).
            blit_affine(
                &mut sub,
                &work,
                shifted,
                layer.effective_opacity.clamp(0.0, 1.0),
                BlendMode::Normal,
                shadow,
                None,
            );
            // Composite sub-region back: single blend vs the live buffer
            // pixels below (exact per-pixel backdrop for every mode).
            for y in 0..sub.h {
                for x in 0..sub.w {
                    let s = sub.px[(y * sub.w + x) as usize];
                    if s.a <= 0.003 {
                        continue;
                    }
                    let dx = x0 + x;
                    let dy = y0 + y;
                    let mut d = dst.px[(dy * ow + dx) as usize];
                    d.blend_over_at(s, layer.blend_mode, dx as i32, dy as i32);
                    dst.px[(dy * ow + dx) as usize] = d;
                }
            }
        }
    }
    dst
}
