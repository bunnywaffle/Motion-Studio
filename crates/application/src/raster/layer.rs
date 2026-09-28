use compositor::{
    AffineTransform2D, BoundingBox2D, EvaluatedEffect, EvaluatedEffectType, EvaluatedLayer,
};
use image::RgbaImage;
use project::{BlendMode, Color, LayerSource, ShapeType, StockPlugin, Vec2};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use super::affine::{Aff, aff_apply, aff_invert, aff_mul, fold_transform, skew_about};
use super::buffer::{FloatBuf, blur_buffer};
use super::effects::{RasterFx, apply_effect_pixels, apply_sharpen, apply_vignette};
use super::mask::apply_masks;
use super::stock::apply_stock;
use super::pixel::{gradient_axis, sample_fill_gradient, Px};
use super::shapes::{
    fill_ellipse, fill_ellipse_gradient, fill_path, fill_path_gradient, fill_rect,
    fill_rect_gradient, stroke_path, stroke_path_gradient,
};
use super::text::{TextSpec, raster_text};

// Layer + composition raster
// ---------------------------------------------------------------------------

/// Everything the rasterizer needs for one frame.
pub struct FrameCtx<'a> {
    pub comp_w: f32,
    pub comp_h: f32,
    pub fit: f32,
    pub time_s: f32,
    pub frame: i64,
    pub playing: bool,
    pub duration_s: f32,
    pub assets: &'a mut HashMap<String, Arc<RgbaImage>>,
}

/// Decoded image cache lookup (shared per panel), keyed by asset id.
pub fn decoded_asset(
    assets: &mut HashMap<String, Arc<RgbaImage>>,
    asset_id: &str,
    path: &std::path::Path,
) -> Option<Arc<RgbaImage>> {
    if let Some(hit) = assets.get(asset_id) {
        return Some(hit.clone());
    }
    let img = image::open(path).ok()?.to_rgba8();
    // Cap decode size for preview speed (long side only, aspect kept).
    let (w, h) = (img.width(), img.height());
    let img = if w.max(h) > 1600 {
        let (nw, nh) = if w >= h {
            (1600, (1600.0 * h as f32 / w as f32).round() as u32)
        } else {
            ((1600.0 * w as f32 / h as f32).round() as u32, 1600)
        };
        image::imageops::resize(&img, nw.max(1), nh.max(1), image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let arc = Arc::new(img);
    assets.insert(asset_id.to_string(), arc.clone());
    Some(arc)
}

/// Rasterize one layer's content in LOCAL coords (unrotated, comp px).
/// Returns the content pixmap plus its local size. Adjustment layers and
/// empty content return None (adjustments post-process the composite).
pub fn raster_content(
    layer: &EvaluatedLayer,
    base_w: f32,
    base_h: f32,
) -> Option<FloatBuf> {
    match &layer.source {
        LayerSource::Solid { color, fill_gradient, .. } => {
            let mut buf = FloatBuf::clear(base_w.ceil().max(1.0) as u32, base_h.ceil().max(1.0) as u32);
            match fill_gradient {
                Some(gradient) => {
                    let axis = gradient_axis(base_w, base_h, gradient.angle);
                    for y in 0..buf.h {
                        for x in 0..buf.w {
                            buf.px[(y * buf.w + x) as usize] = Px::from_color(
                                sample_fill_gradient(gradient, x as f32 + 0.5, y as f32 + 0.5, axis),
                            );
                        }
                    }
                }
                None => {
                    let p = Px::from_color(*color);
                    for px in buf.px.iter_mut() {
                        *px = p;
                    }
                }
            }
            Some(buf)
        }
        LayerSource::Shape { shape_type } => {
            let (w, h) = (base_w.max(2.0), base_h.max(2.0));
            let mut buf = FloatBuf::clear(w.ceil() as u32, h.ceil() as u32);
            match shape_type {
                ShapeType::Rectangle { corner_radius, fill, fill_gradient, .. } => {
                    // Corner radius is stored unscaled; content is unscaled.
                    match fill_gradient {
                        Some(gradient) => fill_rect_gradient(&mut buf, w, h, corner_radius.value, gradient),
                        None => fill_rect(&mut buf, w, h, corner_radius.value, Px::from_color(*fill)),
                    }
                }
                ShapeType::Ellipse { fill, fill_gradient, .. } => {
                    match fill_gradient {
                        Some(gradient) => fill_ellipse_gradient(&mut buf, w / 2.0, h / 2.0, gradient),
                        None => fill_ellipse(&mut buf, w / 2.0, h / 2.0, Px::from_color(*fill)),
                    }
                }
                ShapeType::Path { path_data, fill, fill_gradient, .. } => {
                    let (origin, fw, fh) = path_frame(path_data);
                    match fill_gradient {
                        Some(gradient) => {
                            fill_path_gradient(&mut buf, path_data, gradient, origin, (fw, fh));
                            stroke_path_gradient(&mut buf, path_data, 2.0, gradient, origin, (fw, fh));
                        }
                        None => {
                            fill_path(
                                &mut buf,
                                path_data,
                                Px::from_color(*fill),
                                origin,
                            );
                            stroke_path(
                                &mut buf,
                                path_data,
                                2.0,
                                Px::from_color(*fill),
                                origin,
                            );
                        }
                    }
                }
            }
            Some(buf)
        }
        LayerSource::Text {
            text,
            font_family,
            font_size,
            fill_color,
            fill_gradient,
            weight,
            italic,
            tracking,
            leading,
            align,
            all_caps,
            stroke_width,
            stroke_color,
            stroke_gradient,
            baseline_shift,
            box_width,
            text_path,
            ..
        } => {
            // Effective outline: TextOutline effect wins over native stroke.
            let mut sw = stroke_width.value.max(0.0);
            let mut sc = *stroke_color;
            let mut outline_override = false;
            for eff in &layer.effects {
                if !eff.enabled {
                    continue;
                }
                if let EvaluatedEffectType::TextOutline { width, color } = &eff.effect_type {
                    sw = (*width).max(0.0);
                    sc = *color;
                    outline_override = true;
                    break;
                }
            }
            let mut bevel = None;
            for eff in &layer.effects {
                if !eff.enabled {
                    continue;
                }
                if let EvaluatedEffectType::TextBevel { strength, softness } = &eff.effect_type {
                    if *strength > 0.5 {
                        bevel = Some((*strength, *softness));
                    }
                    break;
                }
            }
            let spec = TextSpec {
                text: &text.value,
                family: font_family,
                size: font_size.value,
                fill: Px::from_color(fill_color.value),
                fill_gradient: fill_gradient.clone(),
                weight: *weight,
                italic: *italic,
                tracking: tracking.value,
                leading: leading.value,
                align: *align,
                all_caps: *all_caps,
                stroke_w: sw,
                stroke_col: Px::from_color(sc),
                // An overriding outline effect replaces the native stroke
                // (and its gradient) with a flat color.
                stroke_gradient: if outline_override { None } else { stroke_gradient.clone() },
                baseline_shift: baseline_shift.value,
                box_w: box_width.value,
                bevel,
            };
            // Text-on-path flows glyphs along the shared path (origin-aware
            // placement); straight text centers ink in the estimate box.
            if let Some(path) = text_path {
                let (tbuf, _ink, (pox, poy)) = super::text::raster_text_on_path(&spec, path);
                let mut out = FloatBuf::clear(base_w.ceil().max(1.0) as u32, base_h.ceil().max(1.0) as u32);
                // Origin-aware placement (no centering): buffer pixel (i,j)
                // is layer-local (ox+i, oy+j), so sample at (x-ox, y-oy).
                for y in 0..out.h {
                    for x in 0..out.w {
                        let s = tbuf.sample(x as f32 - pox, y as f32 - poy);
                        if s.a > 0.003 {
                            out.px[(y * out.w + x) as usize] = s;
                        }
                    }
                }
                return Some(out);
            }
            let (tbuf, ink) = raster_text(&spec);
            // Center the ink in the estimate box (mirrors the old
            // flex-center div layout and the anchor math exactly).
            let (ix0, iy0, ix1, iy1) = ink;
            let mut out = FloatBuf::clear(base_w.ceil().max(1.0) as u32, base_h.ceil().max(1.0) as u32);
            if ix1 > ix0 && iy1 > iy0 {
                let dx = (base_w - (ix1 - ix0)) / 2.0 - ix0;
                let dy = (base_h - (iy1 - iy0)) / 2.0 - iy0;
                for y in 0..out.h {
                    for x in 0..out.w {
                        let s = tbuf.sample(x as f32 - dx, y as f32 - dy);
                        if s.a > 0.003 {
                            out.px[(y * out.w + x) as usize] = s;
                        }
                    }
                }
            }
            Some(out)
        }
        LayerSource::Image { .. } => None, // decoded at blit (native res)
        LayerSource::Video { .. } => {
            // Slate placeholder for footage without a decoder here.
            let mut buf = FloatBuf::clear(base_w.ceil().max(1.0) as u32, base_h.ceil().max(1.0) as u32);
            let p = Px { r: 0.06, g: 0.08, b: 0.16, a: 1.0 };
            for px in buf.px.iter_mut() {
                *px = p;
            }
            Some(buf)
        }
        _ => None,
    }
}

/// Cache key for a layer raster (content + frame + output size).
pub fn layer_cache_key(
    layer: &EvaluatedLayer,
    frame: i64,
    out_w: u32,
    out_h: u32,
    playing: bool,
    backdrop_hash: u64,
) -> u64 {
    let mut h = DefaultHasher::new();
    let is_adjustment = matches!(&layer.source, LayerSource::Adjustment);
    layer.id.hash(&mut h);
    out_w.hash(&mut h);
    out_h.hash(&mut h);
    playing.hash(&mut h);
    if is_adjustment || layer.blend_mode != BlendMode::Normal {
        backdrop_hash.hash(&mut h);
    }
    // Time-varying Shader Lab (`time`/`frame` uniforms) and Adjustment layers
    // (which post-process temporal underlying animations) invalidate per frame;
    // everything else keys off evaluated values below. This avoids
    // the old `format!("{:?}")` whose AST dumps cost milliseconds per
    // layer per tick during drags.
    let time_varying = is_adjustment
        || layer.effects.iter().any(|e| {
            e.enabled && matches!(&e.effect_type, EvaluatedEffectType::ShaderLab { .. })
        });
    (if time_varying { frame } else { 0 }).hash(&mut h);
    // Evaluated transform. Only the linear part (a/b/c/d) affects pixels:
    // pure translation merely shifts the AABB, and the raster is relative
    // to the box origin — so move-drags hit the cache and cost only a
    // GPU-side relayout instead of a re-raster + PNG round-trip. Local
    // position/scale/rotation/anchor are covered transitively: anything
    // that changes pixels also changes the world linear part (or the
    // effects hash below).
    let wm = layer.world_matrix();
    for v in [wm.a, wm.b, wm.c, wm.d] {
        v.to_bits().hash(&mut h);
    }
    if is_adjustment || layer.blend_mode != BlendMode::Normal {
        wm.tx.to_bits().hash(&mut h);
        wm.ty.to_bits().hash(&mut h);
    }
    layer.effective_opacity.to_bits().hash(&mut h);
    (layer.blend_mode as u8).hash(&mut h);
    layer.is_visible.hash(&mut h);
    // Source content.
    match &layer.source {
        LayerSource::Solid { color, width, height, fill_gradient, .. } => {
            0u8.hash(&mut h);
            color_hash(color, &mut h);
            width.hash(&mut h);
            height.hash(&mut h);
            gradient_hash(fill_gradient, &mut h);
        }
        LayerSource::Image { asset_id } => {
            1u8.hash(&mut h);
            asset_id.hash(&mut h);
        }
        LayerSource::Video { asset_id, .. } => {
            2u8.hash(&mut h);
            asset_id.hash(&mut h);
        }
        LayerSource::Text {
            text,
            font_family,
            font_size,
            fill_color,
            fill_gradient,
            weight,
            italic,
            tracking,
            leading,
            align,
            all_caps,
            stroke_width,
            stroke_color,
            stroke_gradient,
            baseline_shift,
            box_width,
            text_path,
            ..
        } => {
            3u8.hash(&mut h);
            text.value.hash(&mut h);
            font_family.hash(&mut h);
            font_size.value.to_bits().hash(&mut h);
            color_hash(&fill_color.value, &mut h);
            gradient_hash(fill_gradient, &mut h);
            weight.hash(&mut h);
            italic.hash(&mut h);
            tracking.value.to_bits().hash(&mut h);
            leading.value.to_bits().hash(&mut h);
            (*align as u8).hash(&mut h);
            all_caps.hash(&mut h);
            stroke_width.value.to_bits().hash(&mut h);
            color_hash(stroke_color, &mut h);
            gradient_hash(stroke_gradient, &mut h);
            baseline_shift.value.to_bits().hash(&mut h);
            box_width.value.to_bits().hash(&mut h);
            if let Some(path) = text_path {
                path.closed.hash(&mut h);
                for pt in &path.points {
                    pt.pos.x.to_bits().hash(&mut h);
                    pt.pos.y.to_bits().hash(&mut h);
                    pt.in_tan.x.to_bits().hash(&mut h);
                    pt.in_tan.y.to_bits().hash(&mut h);
                    pt.out_tan.x.to_bits().hash(&mut h);
                    pt.out_tan.y.to_bits().hash(&mut h);
                    (pt.kind as u8).hash(&mut h);
                }
            }
        }
        LayerSource::Shape { shape_type } => match shape_type {
            ShapeType::Rectangle { width, height, corner_radius, fill, fill_gradient, .. } => {
                4u8.hash(&mut h);
                width.value.to_bits().hash(&mut h);
                height.value.to_bits().hash(&mut h);
                corner_radius.value.to_bits().hash(&mut h);
                color_hash(fill, &mut h);
                gradient_hash(fill_gradient, &mut h);
            }
            ShapeType::Ellipse { radius_x, radius_y, fill, fill_gradient, .. } => {
                5u8.hash(&mut h);
                radius_x.value.to_bits().hash(&mut h);
                radius_y.value.to_bits().hash(&mut h);
                color_hash(fill, &mut h);
                gradient_hash(fill_gradient, &mut h);
            }
            ShapeType::Path { path_data, fill, fill_gradient, .. } => {
                6u8.hash(&mut h);
                path_data.hash(&mut h);
                color_hash(fill, &mut h);
                gradient_hash(fill_gradient, &mut h);
            }
        },
        LayerSource::Adjustment => {
            7u8.hash(&mut h);
        }
        _ => {
            8u8.hash(&mut h);
        }
    }
    // Effects: identity + on/off + scalar/color params.
    for eff in &layer.effects {
        eff.id.hash(&mut h);
        eff.enabled.hash(&mut h);
        effect_hash(&eff.effect_type, &mut h);
    }
    // Masks: identity + on/off + path + shaped params + mode + transform.
    for mask in &layer.masks {
        mask.id.hash(&mut h);
        mask.enabled.hash(&mut h);
        (mask.mode as u8).hash(&mut h);
        for pt in &mask.path.points {
            pt.pos.x.to_bits().hash(&mut h);
            pt.pos.y.to_bits().hash(&mut h);
            pt.in_tan.x.to_bits().hash(&mut h);
            pt.in_tan.y.to_bits().hash(&mut h);
            pt.out_tan.x.to_bits().hash(&mut h);
            pt.out_tan.y.to_bits().hash(&mut h);
            (pt.kind as u8).hash(&mut h);
        }
        mask.path.closed.hash(&mut h);
        mask.opacity.to_bits().hash(&mut h);
        mask.feather.to_bits().hash(&mut h);
        mask.expansion.to_bits().hash(&mut h);
        mask.invert.hash(&mut h);
        let tm = &mask.transform;
        for v in [tm.position.x, tm.position.y, tm.scale.x, tm.scale.y, tm.rotation, tm.anchor_point.x, tm.anchor_point.y] {
            v.to_bits().hash(&mut h);
        }
        let lm = &mask.transform.local_matrix;
        for v in [lm.a, lm.b, lm.c, lm.d, lm.tx, lm.ty] {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn color_hash(c: &Color, h: &mut DefaultHasher) {
    c.r.to_bits().hash(h);
    c.g.to_bits().hash(h);
    c.b.to_bits().hash(h);
    c.a.to_bits().hash(h);
}

fn gradient_hash(g: &Option<project::FillGradient>, h: &mut DefaultHasher) {
    match g {
        Some(grad) => {
            1u8.hash(h);
            grad.angle.to_bits().hash(h);
            for stop in &grad.stops {
                stop.offset.to_bits().hash(h);
                color_hash(&stop.color, h);
            }
        }
        None => 0u8.hash(h),
    }
}

fn effect_hash(fx: &EvaluatedEffectType, h: &mut DefaultHasher) {
    // Discriminant first so types never collide.
    let disc: u8 = match fx {
        EvaluatedEffectType::GaussianBlur { .. } => 1,
        EvaluatedEffectType::BrightnessContrast { .. } => 2,
        EvaluatedEffectType::Tint { .. } => 3,
        EvaluatedEffectType::Invert { .. } => 4,
        EvaluatedEffectType::DropShadow { .. } => 5,
        EvaluatedEffectType::GlslShader { .. } => 6,
        EvaluatedEffectType::DisplacementMap { .. } => 7,
        EvaluatedEffectType::ChromaKey { .. } => 8,
        EvaluatedEffectType::LumaKey { .. } => 9,
        EvaluatedEffectType::NoiseGenerator { .. } => 10,
        EvaluatedEffectType::ShaderLab { .. } => 11,
        EvaluatedEffectType::Checkerboard { .. } => 12,
        EvaluatedEffectType::GradientRamp { .. } => 13,
        EvaluatedEffectType::Perspective { .. } => 14,
        EvaluatedEffectType::TextOutline { .. } => 15,
        EvaluatedEffectType::TextBevel { .. } => 16,
        EvaluatedEffectType::Bloom { .. } => 17,
        EvaluatedEffectType::Tiler { .. } => 18,
        EvaluatedEffectType::Warp { .. } => 19,
        EvaluatedEffectType::Exposure { .. } => 20,
        EvaluatedEffectType::Vibrance { .. } => 21,
        EvaluatedEffectType::Levels { .. } => 22,
        EvaluatedEffectType::HueSaturation { .. } => 23,
        EvaluatedEffectType::Sharpen { .. } => 24,
        EvaluatedEffectType::Vignette { .. } => 25,
        EvaluatedEffectType::Stock { plugin, .. } => 100 + *plugin as u8,
    };
    disc.hash(h);
    match fx {
        EvaluatedEffectType::GaussianBlur { radius } => radius.to_bits().hash(h),
        EvaluatedEffectType::BrightnessContrast { brightness, contrast } => {
            brightness.to_bits().hash(h);
            contrast.to_bits().hash(h);
        }
        EvaluatedEffectType::Tint { map_black, map_white, amount } => {
            color_hash(map_black, h);
            color_hash(map_white, h);
            amount.to_bits().hash(h);
        }
        EvaluatedEffectType::Invert { amount } => amount.to_bits().hash(h),
        EvaluatedEffectType::DropShadow { distance, angle, softness, opacity, color } => {
            distance.to_bits().hash(h);
            angle.to_bits().hash(h);
            softness.to_bits().hash(h);
            opacity.to_bits().hash(h);
            color_hash(color, h);
        }
        EvaluatedEffectType::GlslShader { code, param1, param2, param3, param4 } => {
            code.hash(h);
            param1.to_bits().hash(h);
            param2.to_bits().hash(h);
            param3.to_bits().hash(h);
            param4.to_bits().hash(h);
        }
        EvaluatedEffectType::DisplacementMap { max_horizontal, max_vertical } => {
            max_horizontal.to_bits().hash(h);
            max_vertical.to_bits().hash(h);
        }
        EvaluatedEffectType::ChromaKey { key_color, tolerance, feather } => {
            color_hash(key_color, h);
            tolerance.to_bits().hash(h);
            feather.to_bits().hash(h);
        }
        EvaluatedEffectType::LumaKey { threshold, feather } => {
            threshold.to_bits().hash(h);
            feather.to_bits().hash(h);
        }
        EvaluatedEffectType::NoiseGenerator { amount, monochrome } => {
            amount.to_bits().hash(h);
            monochrome.hash(h);
        }
        EvaluatedEffectType::ShaderLab { source_hash, values, .. } => {
            source_hash.hash(h);
            let mut names: Vec<&String> = values.keys().collect();
            names.sort();
            for n in names {
                n.hash(h);
                shader_value_hash(&values[n], h);
            }
        }
        EvaluatedEffectType::Checkerboard { size, color_a, color_b } => {
            size.to_bits().hash(h);
            color_hash(color_a, h);
            color_hash(color_b, h);
        }
        EvaluatedEffectType::GradientRamp { gradient } => {
            gradient.angle.to_bits().hash(h);
            for stop in &gradient.stops {
                stop.offset.to_bits().hash(h);
                color_hash(&stop.color, h);
            }
        }
        EvaluatedEffectType::Perspective { skew_x, skew_y } => {
            skew_x.to_bits().hash(h);
            skew_y.to_bits().hash(h);
        }
        EvaluatedEffectType::TextOutline { width, color } => {
            width.to_bits().hash(h);
            color_hash(color, h);
        }
        EvaluatedEffectType::TextBevel { strength, softness } => {
            strength.to_bits().hash(h);
            softness.to_bits().hash(h);
        }
        EvaluatedEffectType::Bloom { intensity, radius } => {
            intensity.to_bits().hash(h);
            radius.to_bits().hash(h);
        }
        EvaluatedEffectType::Tiler { tiles_x, tiles_y } => {
            tiles_x.to_bits().hash(h);
            tiles_y.to_bits().hash(h);
        }
        EvaluatedEffectType::Warp { amount, scale } => {
            amount.to_bits().hash(h);
            scale.to_bits().hash(h);
        }
        EvaluatedEffectType::Exposure { exposure } => exposure.to_bits().hash(h),
        EvaluatedEffectType::Vibrance { vibrance } => vibrance.to_bits().hash(h),
        EvaluatedEffectType::Levels { input_black, input_white, gamma, output_black, output_white } => {
            input_black.to_bits().hash(h);
            input_white.to_bits().hash(h);
            gamma.to_bits().hash(h);
            output_black.to_bits().hash(h);
            output_white.to_bits().hash(h);
        }
        EvaluatedEffectType::HueSaturation { hue_shift, saturation, lightness } => {
            hue_shift.to_bits().hash(h);
            saturation.to_bits().hash(h);
            lightness.to_bits().hash(h);
        }
        EvaluatedEffectType::Sharpen { amount, radius } => {
            amount.to_bits().hash(h);
            radius.to_bits().hash(h);
        }
        EvaluatedEffectType::Vignette { amount, softness } => {
            amount.to_bits().hash(h);
            softness.to_bits().hash(h);
        }
        EvaluatedEffectType::Stock { params, colors, .. } => {
            params.len().hash(h);
            for v in params {
                v.to_bits().hash(h);
            }
            for c in colors {
                color_hash(c, h);
            }
        }
    }
}

fn shader_value_hash(v: &project::ShaderParamValue, h: &mut DefaultHasher) {
    use project::ShaderParamValue as SPV;
    match v {
        SPV::Float(x) => {
            0u8.hash(h);
            x.to_bits().hash(h);
        }
        SPV::Int(x) => {
            1u8.hash(h);
            x.hash(h);
        }
        SPV::Bool(x) => {
            2u8.hash(h);
            x.hash(h);
        }
        SPV::Vec2(a) => {
            3u8.hash(h);
            for x in a {
                x.to_bits().hash(h);
            }
        }
        SPV::Vec3(a) => {
            4u8.hash(h);
            for x in a {
                x.to_bits().hash(h);
            }
        }
        SPV::Vec4(a) => {
            5u8.hash(h);
            for x in a {
                x.to_bits().hash(h);
            }
        }
        SPV::Color(c) => {
            6u8.hash(h);
            color_hash(c, h);
        }
    }
}

/// Cached per-layer raster for the viewport.
#[derive(Clone)]
pub struct RasterEntry {
    pub key: u64,
    /// Straight-alpha BGRA8 bytes sized w*h (feeds `RenderImage` directly —
    /// no PNG encode/decode round-trip on the display path).
    pub bgra: Arc<Vec<u8>>,
    pub w: u32,
    pub h: u32,
    pub avg: Color,
    pub empty: bool,
}

/// Rasterize one layer into its AABB box (`out_w` x `out_h`, canvas px).
/// Returns the pixmap plus its straight average color. Adjustment layers
/// and fully transparent results yield `empty`.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_layer(
    layer: &EvaluatedLayer,
    base_w: f32,
    base_h: f32,
    out_w: u32,
    out_h: u32,
    comp_w: f32,
    comp_h: f32,
    backdrop: Color,
    backdrop_buf: Option<&FloatBuf>,
    time_s: f32,
    frame: i64,
    playing: bool,
    duration_s: f32,
    assets: &HashMap<String, Arc<RgbaImage>>,
) -> (FloatBuf, Color, bool) {
    let (ow, oh) = (out_w.max(1), out_h.max(1));
    let mut out = FloatBuf::clear(ow, oh);
    if matches!(&layer.source, LayerSource::Adjustment) {
        // If the adjustment layer has zero enabled effects and normal blend mode,
        // it applies no transformations to the composite. Return empty = true so
        // that underlying layers render cleanly and natively without an opaque pass-through bitmap.
        let has_active_fx = layer.effects.iter().any(|e| e.enabled);
        if !has_active_fx && layer.blend_mode == BlendMode::Normal {
            return (out, Color::TRANSPARENT, true);
        }

        let fx = RasterFx {
            time_s,
            frame,
            res_w: comp_w,
            res_h: comp_h,
            duration_s,
            playing,
        };
        // 1. Initial backdrop buffer (either sliced beneath layer, or fallback backdrop color)
        let mut work = if let Some(b) = backdrop_buf {
            if b.w == ow && b.h == oh {
                b.clone()
            } else {
                let mut resized = FloatBuf::clear(ow, oh);
                for y in 0..oh {
                    let sy = (y as f32 / oh as f32) * b.h as f32;
                    for x in 0..ow {
                        let sx = (x as f32 / ow as f32) * b.w as f32;
                        resized.put(x as i32, y as i32, b.sample(sx, sy));
                    }
                }
                resized
            }
        } else {
            let mut fill = FloatBuf::clear(ow, oh);
            let p = Px::from_color(backdrop);
            for px in fill.px.iter_mut() {
                *px = p;
            }
            fill
        };

        let pristine = work.clone();

        // 2. Post-process the backdrop with layer's effects
        apply_adjustment(&mut work, &layer.effects, &fx);

        // 3. Evaluate masks (if any)
        // Masks on an adjustment layer define WHERE the adjustment applies.
        // Inside mask -> adjusted; outside mask -> pristine backdrop.
        let mask_cov = if !layer.masks.is_empty() {
            let sx = ow as f32 / base_w.max(1.0);
            let sy = oh as f32 / base_h.max(1.0);
            let scale_xform = AffineTransform2D::from_scale(Vec2::new(sx, sy));
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

        // 4. Blend adjusted result over pristine backdrop using mask coverage, opacity, and blend mode
        let eff_op = layer.effective_opacity.clamp(0.0, 1.0);
        for y in 0..oh {
            for x in 0..ow {
                let idx = (y * ow + x) as usize;
                let orig = pristine.px[idx];
                let adj = work.px[idx];
                let m = mask_cov.as_ref().map(|cov| cov[idx]).unwrap_or(1.0);
                let alpha = (m * eff_op).clamp(0.0, 1.0);

                if alpha <= 0.001 {
                    out.px[idx] = Px::clear();
                    continue;
                }

                let target_px = if layer.blend_mode == BlendMode::Normal {
                    adj
                } else {
                    let mut b = orig;
                    b.blend_over_at(adj, layer.blend_mode, x as i32, y as i32);
                    b
                };

                out.px[idx] = Px {
                    r: target_px.r * alpha,
                    g: target_px.g * alpha,
                    b: target_px.b * alpha,
                    a: target_px.a * alpha,
                };
            }
        }

        let avg = out.average();
        return (out, avg, false);
    }
    // World map (local -> output px of this AABB box).
    //
    // The output box IS the layer AABB (the viewer shell draws this exact
    // box, stretched to canvas scale), so the local->output scale is
    // `out / bbox` — NOT `out / comp`. Using comp width here shrank every
    // layer's pixels inside its gizmo (e.g. a 300px solid in a 1920px comp
    // rendered ~6x too small).
    // World box from the layer's LOCAL content box. Pen/path shapes live in
    // arbitrary local coords (see `path_frame`), so the box starts at the
    // frame origin — not at (0, 0). `frame_ox/oy` re-bases the world map
    // below so buffer px (0, 0) means local `frame origin`.
    let local_box = layer_local_box(layer, base_w, base_h);
    let (frame_ox, frame_oy) = (local_box.min.x, local_box.min.y);
    let bbox = layer.local_to_world_bbox(&local_box);
    let bw = (bbox.max.x - bbox.min.x).max(1e-3);
    let bh = (bbox.max.y - bbox.min.y).max(1e-3);
    let kx = ow as f32 / bw;
    let ky = oh as f32 / bh;
    let fx = RasterFx {
        time_s,
        frame,
        res_w: comp_w,
        res_h: comp_h,
        duration_s,
        playing,
    };
    let Some(content) = raster_layer_content(layer, base_w, base_h, assets) else {
        return (out, backdrop, true);
    };
    // Local effects (masks first — After Effects order: the mask stack
    // shapes alpha before any effect sees pixels). On framed (path) layers
    // the work buffer is frame-relative, so mask paths shift by `-origin`.
    let mut work = content;
    // Fast preview (playback/gestures): evaluate shaped masks at output
    // res with radii scaled by the output/work ratio, then upscale the
    // smooth coverage — full-res filtering per frame is what stalls mask
    // drags and feathered playback. Idle keeps the exact work-space path.
    let out_scale =
        ((ow as f32 / work.w.max(1) as f32) + (oh as f32 / work.h.max(1) as f32)) * 0.5;
    if playing && out_scale < 0.75 && !layer.masks.is_empty() {
        let wm = layer.world_matrix();
        // Work px -> output px (pre-effect base map; masks apply before
        // effects, so perspective/stock folds stay out of it).
        let base = AffineTransform2D {
            a: wm.a * kx,
            b: wm.b * ky,
            c: wm.c * kx,
            d: wm.d * ky,
            tx: ((wm.a * frame_ox + wm.c * frame_oy + wm.tx) - bbox.min.x) * kx,
            ty: ((wm.b * frame_ox + wm.d * frame_oy + wm.ty) - bbox.min.y) * ky,
        };
        let shift = AffineTransform2D::from_translation(Vec2::new(-frame_ox, -frame_oy));
        let space = base * shift;
        if let Some(cov) =
            crate::raster::mask::evaluate_mask_coverage_mapped(ow, oh, &layer.masks, &space, out_scale)
        {
            crate::raster::mask::apply_scaled_coverage(&mut work, &cov, ow, oh);
        }
    } else if frame_ox != 0.0 || frame_oy != 0.0 {
        let shift = AffineTransform2D::from_translation(Vec2::new(-frame_ox, -frame_oy));
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
    apply_layer_fx(&mut work, base_w, base_h, &layer.effects, &fx);
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
        // Blur runs in work-buffer (local px) space, before the world map,
        // so the radius applies unscaled — it is a content-space value.
        blur_buffer(&mut work, blur_total);
    }
    if let Some((intensity, radius)) = bloom {
        apply_bloom(&mut work, intensity, radius);
    }
    // World map (frame px -> output px of this AABB box). Buffer (0, 0) is
    // local `frame origin`, so the translation carries `W * origin`.
    let wm = layer.world_matrix();
    let mut pmap = Aff {
        a: wm.a * kx,
        b: wm.b * ky,
        c: wm.c * kx,
        d: wm.d * ky,
        tx: ((wm.a * frame_ox + wm.c * frame_oy + wm.tx) - bbox.min.x) * kx,
        ty: ((wm.b * frame_ox + wm.d * frame_oy + wm.ty) - bbox.min.y) * ky,
    };
    // Perspective skew folds into the map (same as the full-comp path:
    // the skew runs first in local px, the world map scales after it).
    for eff in &layer.effects {
        if !eff.enabled {
            continue;
        }
        if let EvaluatedEffectType::Perspective { skew_x, skew_y } = &eff.effect_type {
            if skew_x.abs() >= 0.05 || skew_y.abs() >= 0.05 {
                pmap = aff_mul(
                    pmap,
                    skew_about(*skew_x, *skew_y, base_w / 2.0, base_h / 2.0),
                );
            }
            break;
        }
    }
    let mut shifted = pmap;
    // Transform stock plug-in folds into the map (translate in output px,
    // scale/rotate about the AABB center).
    for eff in &layer.effects {
        if !eff.enabled {
            continue;
        }
        if let EvaluatedEffectType::Stock { plugin, params, .. } = &eff.effect_type {
            if *plugin == StockPlugin::TransformFx {
                use compositor::fx::stock_p;
                shifted = fold_transform(
                    shifted,
                    ow as f32 * 0.5,
                    oh as f32 * 0.5,
                    stock_p(*plugin, params, 0) * kx,
                    stock_p(*plugin, params, 1) * ky,
                    stock_p(*plugin, params, 2),
                    stock_p(*plugin, params, 3),
                );
            }
        }
    }
    // Drop shadow (softness blurs the silhouette once, up front).
    let mut shadow = None;
    let mut shadow_blurred: Option<FloatBuf> = None;
    for eff in &layer.effects {
        if !eff.enabled {
            continue;
        }
        if let EvaluatedEffectType::DropShadow { distance, angle, softness, opacity, color } =
            &eff.effect_type
        {
            let rad = angle.to_radians();
            shadow = Some((
                distance * rad.cos() * kx,
                distance * rad.sin() * ky,
                (opacity / 100.0).clamp(0.0, 1.0) * 0.75,
                *color,
            ));
            if *softness > 0.5 {
                // Alpha-only silhouette, blurred in work px.
                let mut sil = FloatBuf::clear(work.w, work.h);
                for (d, s) in sil.px.iter_mut().zip(work.px.iter()) {
                    *d = Px { r: 0.0, g: 0.0, b: 0.0, a: s.a };
                }
                blur_buffer(&mut sil, *softness);
                shadow_blurred = Some(sil);
            }
            break;
        }
    }
    let normal = layer.blend_mode == BlendMode::Normal;
    if normal {
        blit_affine(
            &mut out,
            &work,
            shifted,
            layer.effective_opacity.clamp(0.0, 1.0),
            BlendMode::Normal,
            shadow,
            shadow_blurred.as_ref(),
        );
    } else {
        // Exotic modes blend against the sampled backdrop average (the
        // viewport stacks isolated per-layer divs, so there is no live
        // per-pixel backdrop here — the export path in comp.rs does the
        // exact per-pixel composite). Blit ISOLATED first (transparent
        // backdrop), then blend each covered pixel once vs the backdrop:
        // pre-compositing over the backdrop and blending again would feed
        // `blend(backdrop, src_over_backdrop)` instead of
        // `blend(backdrop, src)` and wash every exotic mode out.
        // Uncovered pixels stay transparent so the real layers below show
        // through the div stack (blending transparent src is identity).
        let mut tmp = FloatBuf::clear(ow, oh);
        blit_affine(
            &mut tmp,
            &work,
            shifted,
            layer.effective_opacity.clamp(0.0, 1.0),
            BlendMode::Normal,
            shadow,
            shadow_blurred.as_ref(),
        );
        let mut bg = FloatBuf::clear(ow, oh);
        for (i, s) in tmp.px.iter().enumerate() {
            if s.a <= 0.003 {
                continue;
            }
            // Backdrop blend is positional (dissolve dither stability).
            let x = (i as u32 % bg.w) as i32;
            let y = (i as u32 / bg.w) as i32;
            let mut d = if let Some(bb) = backdrop_buf {
                bb.get(x, y)
            } else {
                Px::from_color(backdrop)
            };
            d.blend_over_at(*s, layer.blend_mode, x, y);
            bg.px[i] = d;
        }
        out = bg;
    }
    let avg = out.average();
    let empty = avg.a < 0.004;
    (out, avg, empty)
}

/// Blit `src` into `dst` through an affine local->dst map with bilinear
/// sampling, opacity, and blend mode. `shadow` draws a blurred offset
/// silhouette underneath first (drop shadow); `shadow_src` optionally
/// overrides the silhouette shape (pre-softened alpha buffer).
pub fn blit_affine(
    dst: &mut FloatBuf,
    src: &FloatBuf,
    map: Aff,
    opacity: f32,
    blend: BlendMode,
    shadow: Option<(f32, f32, f32, Color)>,
    shadow_src: Option<&FloatBuf>,
) {
    let inv = match aff_invert(map) {
        Some(m) => m,
        None => return,
    };
    let op = opacity.clamp(0.0, 1.0);
    // Drop shadow silhouettes first.
    if let Some((sh_dx, sh_dy, sh_alpha, sh_color)) = shadow {
        if sh_alpha > 0.01 {
            // Softened silhouette when provided, else the sharp source.
            let sil = shadow_src.unwrap_or(src);
            // The silhouette buffer lives in work px; map it through the
            // same transform by scaling sample coords into its space.
            let sx = sil.w as f32 / src.w.max(1) as f32;
            let sy = sil.h as f32 / src.h.max(1) as f32;
            for y in 0..dst.h {
                for x in 0..dst.w {
                    let (u, v) = aff_apply(inv, x as f32 - sh_dx, y as f32 - sh_dy);
                    let s = sil.sample(u * sx, v * sy);
                    if s.a > 0.01 {
                        let a = (s.a * sh_alpha * op).clamp(0.0, 1.0);
                        let p = Px {
                            r: sh_color.r * a,
                            g: sh_color.g * a,
                            b: sh_color.b * a,
                            a,
                        };
                        let dstp = dst.get(x as i32, y as i32);
                        let mut out = dstp;
                        out.blend_over(p, BlendMode::Normal);
                        dst.put(x as i32, y as i32, out);
                    }
                }
            }
        }
    }
    for y in 0..dst.h {
        for x in 0..dst.w {
            let (u, v) = aff_apply(inv, x as f32, y as f32);
            let mut s = src.sample(u, v);
            if s.a <= 0.003 {
                continue;
            }
            s.scale(op);
            let dstp = dst.get(x as i32, y as i32);
            let mut out = dstp;
            // Dissolve dithers inside (spatially stable speckle).
            out.blend_over_at(s, blend, x as i32, y as i32);
            dst.put(x as i32, y as i32, out);
        }
    }
}

/// Base content dims (mirror the viewer estimate for pivot consistency).
/// Raster frame for a pen/path shape layer: `(origin, w, h)` in layer-local
/// coords. The content buffer spans `origin .. origin + (w, h)` (tight path
/// bounds plus pad); drawing subtracts `origin` and world boxes are built
/// from this frame. Non-path layers use `(ZERO, base_w, base_h)`.
pub(crate) fn path_frame(path_data: &str) -> (Vec2, f32, f32) {
    match project::Path::from_svg(path_data).frame(8.0) {
        Some((origin, size)) => (origin, size.x, size.y),
        None => (Vec2::ZERO, 400.0, 300.0),
    }
}

/// Local content box for a layer: path shapes use their raster frame,
/// everything else spans `(0, 0, base_w, base_h)`. Shared by the rasterizer,
/// the viewer shells and the transform gizmo so pixels, hit areas and
/// handles always agree (pen paths live in arbitrary local coords).
pub fn layer_local_box(layer: &EvaluatedLayer, base_w: f32, base_h: f32) -> BoundingBox2D {
    match &layer.source {
        LayerSource::Shape { shape_type: ShapeType::Path { path_data, .. } } => {
            let (origin, w, h) = path_frame(path_data);
            BoundingBox2D::from_origin_size(origin, Vec2::new(w, h))
        }
        _ => BoundingBox2D::from_origin_size(Vec2::ZERO, Vec2::new(base_w, base_h)),
    }
}

/// Gizmo corner handles in layer-local coords: the corners of the
/// origin-aware content box (pen paths live at the frame origin, not
/// `(0, 0)`). The viewer maps these through the world matrix, so handles
/// sit on the shell box for every layer kind.
pub fn gizmo_local_corners(layer: &EvaluatedLayer, base_w: f32, base_h: f32) -> [Vec2; 4] {
    let b = layer_local_box(layer, base_w, base_h);
    [
        Vec2::new(b.min.x, b.min.y),
        Vec2::new(b.max.x, b.min.y),
        Vec2::new(b.max.x, b.max.y),
        Vec2::new(b.min.x, b.max.y),
    ]
}

pub(crate) fn layer_base_dims(
    layer: &EvaluatedLayer,
    comp_w: f32,
    comp_h: f32,
    assets: &HashMap<String, Arc<RgbaImage>>,
) -> (f32, f32) {
    match &layer.source {
        LayerSource::Solid { width, height, .. } => (*width as f32, *height as f32),
        LayerSource::Image { asset_id } => {
            // Prefer the evaluated content size when known, else asset dims.
            if let Some(img) = assets.get(asset_id) {
                (img.width() as f32, img.height() as f32)
            } else {
                (1920.0, 1080.0)
            }
        }
        LayerSource::Video { .. } => (1920.0, 1080.0),
        LayerSource::Text { text, font_size, text_path, .. } => {
            if let Some(path) = text_path {
                // Size origin-inclusive so the path-placed glyphs (which
                // span from the local origin) are not clipped.
                if let Some((mn, mx)) = path.bounds() {
                    let pad = font_size.value * 1.5 + 16.0;
                    let x0 = mn.x.min(0.0) - pad;
                    let y0 = mn.y.min(0.0) - pad;
                    let x1 = mx.x.max(0.0) + pad;
                    let y1 = mx.y.max(0.0) + pad;
                    return ((x1 - x0).max(64.0), (y1 - y0).max(64.0));
                }
            }
            let len = text.value.chars().count().max(1) as f32;
            let fs = font_size.value;
            ((len * fs * 0.6 + 40.0).max(100.0), (fs * 1.4 + 20.0).max(40.0))
        }
        LayerSource::Shape { shape_type } => match shape_type {
            ShapeType::Rectangle { width, height, .. } => (width.value, height.value),
            ShapeType::Ellipse { radius_x, radius_y, .. } => {
                (radius_x.value * 2.0, radius_y.value * 2.0)
            }
            ShapeType::Path { path_data, .. } => {
                // Frame-aware bounds (see `path_frame`): pen paths live in
                // arbitrary local coords, not inside (0, 0, size).
                let (_, w, h) = path_frame(path_data);
                (w, h)
            }
        },
        _ => (comp_w, comp_h),
    }
}

pub(crate) fn raster_layer_content(
    layer: &EvaluatedLayer,
    base_w: f32,
    base_h: f32,
    assets: &HashMap<String, Arc<RgbaImage>>,
) -> Option<FloatBuf> {
    match &layer.source {
        LayerSource::Image { asset_id } => {
            // Decoded by the caller into `assets` keyed by asset id.
            assets.get(asset_id).map(|img| {
                let (w, h) = (img.width(), img.height());
                let mut buf = FloatBuf::clear(w.max(1), h.max(1));
                for y in 0..h {
                    for x in 0..w {
                        let p = img.get_pixel(x, y);
                        buf.px[(y * w + x) as usize] = Px {
                            r: p[0] as f32 / 255.0,
                            g: p[1] as f32 / 255.0,
                            b: p[2] as f32 / 255.0,
                            a: p[3] as f32 / 255.0,
                        };
                    }
                }
                buf
            })
        }
        _ => raster_content(layer, base_w, base_h),
    }
}

pub(crate) fn apply_layer_fx(
    buf: &mut FloatBuf,
    base_w: f32,
    base_h: f32,
    effects: &[EvaluatedEffect],
    fx: &RasterFx,
) {
    for eff in effects {
        if !eff.enabled {
            continue;
        }
        match &eff.effect_type {
            EvaluatedEffectType::GaussianBlur { .. }
            | EvaluatedEffectType::DropShadow { .. }
            | EvaluatedEffectType::Bloom { .. }
            | EvaluatedEffectType::Perspective { .. }
            | EvaluatedEffectType::TextOutline { .. }
            | EvaluatedEffectType::TextBevel { .. } => {
                // Handled at raster/blur/blit stages.
            }
            other => apply_effect_pixels(buf, base_w, base_h, other, fx),
        }
    }
}

pub(crate) fn apply_bloom(buf: &mut FloatBuf, intensity: f32, radius_px: f32) {
    let k = (intensity / 100.0).clamp(0.0, 1.0);
    if k <= 0.0 {
        return;
    }
    // Bright-pass copy.
    let mut bright = FloatBuf::clear(buf.w, buf.h);
    for (i, p) in buf.px.iter().enumerate() {
        let lum = 0.299 * p.r + 0.587 * p.g + 0.114 * p.b;
        if lum > 0.45 {
            bright.px[i] = *p;
        }
    }
    blur_buffer(&mut bright, radius_px.max(1.0));
    // Screen the glow back (straight-space math, premultiplied store).
    for (d, g) in buf.px.iter_mut().zip(bright.px.iter()) {
        if g.a <= 0.0 || d.a <= 0.0 {
            continue;
        }
        let (dr, dg, db) = (d.r / d.a, d.g / d.a, d.b / d.a);
        let (gr, gg, gb) = (g.r / g.a, g.g / g.a, g.b / g.a);
        let s = |x: f32, y: f32| 1.0 - (1.0 - x) * (1.0 - y * k);
        d.r = s(dr, gr).clamp(0.0, 1.0) * d.a;
        d.g = s(dg, gg).clamp(0.0, 1.0) * d.a;
        d.b = s(db, gb).clamp(0.0, 1.0) * d.a;
    }
}

/// Adjustment layer: post-process the composite beneath it.
pub(crate) fn apply_adjustment(buf: &mut FloatBuf, effects: &[EvaluatedEffect], fx: &RasterFx) {
    let mut blur_total = 0.0f32;
    let mut sharpen: Option<(f32, f32)> = None;
    let mut vignette: Option<(f32, f32)> = None;
    for eff in effects {
        if !eff.enabled {
            continue;
        }
        match &eff.effect_type {
            EvaluatedEffectType::GaussianBlur { radius } => blur_total += *radius,
            EvaluatedEffectType::Sharpen { amount, radius } => {
                if *amount > 0.5 {
                    sharpen = Some((*amount, *radius));
                }
            }
            EvaluatedEffectType::Vignette { amount, softness }
                if *amount > 0.05 => {
                    vignette = Some((*amount, *softness));
                }
            _ => {}
        }
    }
    if blur_total > 0.25 {
        blur_buffer(buf, blur_total);
    }
    if let Some((amount, radius)) = sharpen {
        apply_sharpen(buf, amount, radius);
    }
    if let Some((amount, softness)) = vignette {
        apply_vignette(buf, amount, softness);
    }
    // Spatial stock plug-ins resolve at buffer level; per-pixel stock
    // resolves through process_color in the loop below.
    for eff in effects {
        if !eff.enabled {
            continue;
        }
        if let EvaluatedEffectType::Stock { plugin, params, colors } = &eff.effect_type {
            if plugin.descriptor().spatial {
                apply_stock(buf, *plugin, params, colors, fx);
            }
        }
    }
    for eff in effects {
        if !eff.enabled {
            continue;
        }
        match &eff.effect_type {
            EvaluatedEffectType::GaussianBlur { .. } => {}
            other => {
                for p in buf.px.iter_mut() {
                    if p.a <= 0.0 {
                        continue;
                    }
                    let c = other.process_color(p.to_color());
                    // Keyed alpha applies (premultiplied store).
                    *p = Px::from_color(c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{LayerStackEvaluator, SceneGraph};
    use project::{Composition, Project, TimeCode};

    fn solid_layer() -> (Project, String) {
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let layer = project::Layer::solid("l1", "L", Color::WHITE, 100, 100, tc, tc);
        comp.add_layer(layer).unwrap();
        project.add_composition(comp).unwrap();
        (project, "c".to_string())
    }

    fn eval_first(project: &Project, comp_id: &str) -> compositor::EvaluatedLayer {
        let graph = SceneGraph::from_project(project, comp_id).expect("graph");
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(0, 30.0));
        stack.get_layer("l1").expect("layer").clone()
    }

    #[test]
    fn fingerprint_stable_and_sensitive() {
        let (project, comp_id) = solid_layer();
        let layer = eval_first(&project, &comp_id);
        let k1 = layer_cache_key(&layer, 0, 100, 100, false, 0);
        // Same inputs -> same key (cache hit, no Debug formatting).
        assert_eq!(k1, layer_cache_key(&layer, 0, 100, 100, false, 0));
        // Static layers ignore the frame (cross-frame hits during playback).
        assert_eq!(k1, layer_cache_key(&layer, 99, 100, 100, false, 0));
        // Quality flag participates.
        assert_ne!(k1, layer_cache_key(&layer, 0, 100, 100, true, 0));
        // Size participates.
        assert_ne!(k1, layer_cache_key(&layer, 0, 50, 50, false, 0));
        // Pure translation does NOT change the key: the raster is relative
        // to the box origin, so move-drags hit the cache and cost only a
        // relayout. Rotation changes the linear part, so it must miss.
        // (Evaluated through the real evaluator so world matrices match
        // production; mutating EvaluatedTransform fields by hand would not
        // refresh the stored world matrix.)
        let translated = {
            let mut project = Project::new("p", "P");
            let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
            let tc = TimeCode::from_frames(0, 30.0);
            let mut layer = project::Layer::solid("l1", "L", Color::WHITE, 100, 100, tc, tc);
            layer.transform.position.set_value(project::Vec2::new(111.0, -37.0));
            comp.add_layer(layer).unwrap();
            project.add_composition(comp).unwrap();
            eval_first(&project, "c")
        };
        assert_eq!(k1, layer_cache_key(&translated, 0, 100, 100, false, 0));
        let rotated = {
            let mut project = Project::new("p", "P");
            let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
            let tc = TimeCode::from_frames(0, 30.0);
            let mut layer = project::Layer::solid("l1", "L", Color::WHITE, 100, 100, tc, tc);
            layer.transform.rotation.set_value(23.0);
            comp.add_layer(layer).unwrap();
            project.add_composition(comp).unwrap();
            eval_first(&project, "c")
        };
        assert_ne!(k1, layer_cache_key(&rotated, 0, 100, 100, false, 0));
    }

    fn path_project() -> (Project, String) {
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        // Pen path in arbitrary local coords (negative + far from origin):
        // the old (0, 0, size) box clipped everything outside it.
        let layer = project::Layer::shape(
            "l1",
            "Pen",
            project::ShapeType::Path {
                path_data: "M -100.0 -50.0 L 100.0 60.0".to_string(),
                fill: Color::WHITE,
                fill_gradient: None,
            },
            tc,
            out,
        );
        comp.add_layer(layer).unwrap();
        project.add_composition(comp).unwrap();
        (project, "c".to_string())
    }

    #[test]
    fn gizmo_corners_use_frame_origin_for_paths() {
        // Pen path in negative coords: gizmo corners must sit on the frame
        // origin box (same box the shell and rasterizer use), not (0, 0).
        let (project, comp_id) = path_project();
        let layer = eval_first(&project, &comp_id);
        let (base_w, base_h) = (216.0, 126.0);
        let corners = gizmo_local_corners(&layer, base_w, base_h);
        let frame = layer_local_box(&layer, base_w, base_h);
        assert!(frame.min.x < 0.0 && frame.min.y < 0.0, "{frame:?}");
        assert_eq!(corners[0], frame.min);
        assert_eq!(corners[2], frame.max);
        // Solid layers keep the classic (0, 0, base) box.
        let (project, comp_id) = solid_layer();
        let layer = eval_first(&project, &comp_id);
        let corners = gizmo_local_corners(&layer, 100.0, 100.0);
        assert_eq!(corners[0], project::Vec2::new(0.0, 0.0));
        assert_eq!(corners[2], project::Vec2::new(100.0, 100.0));
    }

    #[test]
    fn path_frame_raster_covers_negative_coords() {
        use std::collections::HashMap;
        let (project, comp_id) = path_project();
        let layer = eval_first(&project, &comp_id);
        let assets: HashMap<String, std::sync::Arc<image::RgbaImage>> = HashMap::new();
        let (base_w, base_h) = layer_base_dims(&layer, 1920.0, 1080.0, &assets);
        // Frame covers the whole stroke plus pad.
        assert!((base_w - 216.0).abs() < 1e-3, "{base_w}");
        assert!((base_h - 126.0).abs() < 1e-3, "{base_h}");
        // World box has real area (no collapse to a clipped sliver).
        let bbox = layer.local_to_world_bbox(&layer_local_box(&layer, base_w, base_h));
        assert!(bbox.width() > 200.0 && bbox.height() > 110.0, "{bbox:?}");
        // And the raster actually holds ink (not an empty/clipped buffer).
        let (buf, _avg, empty) = rasterize_layer(
            &layer,
            base_w,
            base_h,
            base_w.ceil() as u32,
            base_h.ceil() as u32,
            1920.0,
            1080.0,
            Color::BLACK,
            None,
            0.0,
            0,
            false,
            5.0,
            &assets,
        );
        assert!(!empty, "negative-coord pen path must rasterize ink");
        assert!(buf.px.iter().any(|p| p.a > 0.05));
    }

    #[test]
    fn parented_shape_stays_rendered_with_blend() {
        use project::BlendMode;
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        let bg = project::Layer::solid("bg", "BG", Color::WHITE, 1920, 1080, tc, out);
        comp.add_layer(bg).unwrap();
        let mut shape = project::Layer::shape(
            "l1",
            "Rect",
            project::ShapeType::Rectangle {
                width: project::Property::new("W", 200.0),
                height: project::Property::new("H", 100.0),
                corner_radius: project::Property::new("R", 0.0),
                fill: Color::rgb(1.0, 0.0, 0.0), fill_gradient: None,
            },
            tc,
            out,
        );
        shape.blend_mode = BlendMode::Multiply;
        shape.parent_id = Some("bg".to_string());
        comp.add_layer(shape).unwrap();
        project.add_composition(comp).unwrap();
        let graph = SceneGraph::from_project(&project, "c").expect("graph");
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &TimeCode::from_frames(0, 30.0));
        let ids: Vec<&str> = stack.render_layers().iter().map(|l| l.id.as_str()).collect();
        assert!(ids.contains(&"l1"), "parented shape must stay rendered: {ids:?}");
        // Multiply red over white is red (single blend application — the old
        // export path blended twice, vs transparent black first, and came
        // out black).
        let mut dst = crate::raster::buffer::FloatBuf::clear(4, 4);
        for px in dst.px.iter_mut() {
            *px = crate::raster::pixel::Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let src = crate::raster::pixel::Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let mut d = dst.px[0];
        d.blend_over_at(src, BlendMode::Multiply, 0, 0);
        assert!((d.r - 1.0).abs() < 1e-4 && d.g.abs() < 1e-4 && d.b.abs() < 1e-4);
    }

    fn masked_solid_project(mode: project::MaskMode, invert: bool) -> (Project, String) {
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        let mut layer = project::Layer::solid("l1", "L", Color::WHITE, 100, 100, tc, out);
        let mut mask = project::Mask::with_path(
            "m1",
            "M",
            project::Path::rectangle(0.0, 0.0, 50.0, 100.0),
        );
        mask.mode = mode;
        mask.invert = invert;
        layer.masks.push(mask);
        comp.add_layer(layer).unwrap();
        project.add_composition(comp).unwrap();
        (project, "c".to_string())
    }

    fn rasterize_l1(project: &Project, comp_id: &str) -> (FloatBuf, bool) {
        use std::collections::HashMap;
        let layer = eval_first(project, comp_id);
        let assets: HashMap<String, std::sync::Arc<image::RgbaImage>> = HashMap::new();
        let (buf, _avg, empty) = rasterize_layer(
            &layer, 100.0, 100.0, 100, 100, 1920.0, 1080.0, Color::BLACK, None, 0.0, 0,
            false, 5.0, &assets,
        );
        (buf, empty)
    }

    #[test]
    fn masked_solid_add_keeps_inside_transparent_outside() {
        let (project, comp_id) = masked_solid_project(project::MaskMode::Add, false);
        let (buf, empty) = rasterize_l1(&project, &comp_id);
        assert!(!empty, "masked solid must hold ink");
        // Inside the mask (left half): opaque white.
        let inside = buf.get(25, 50);
        assert!(inside.a > 0.9 && inside.r > 0.9, "{inside:?}");
        // Outside the mask (right half): transparent (no black-opaque artifact).
        assert!(buf.get(75, 50).a < 0.01, "{:?}", buf.get(75, 50));
        // Every surviving pixel is white: no black artifacts anywhere.
        for p in buf.px.iter().filter(|p| p.a > 0.5) {
            assert!(p.r > 0.9 && p.g > 0.9 && p.b > 0.9, "{p:?}");
        }
    }

    #[test]
    fn masked_solid_subtract_and_invert_mirror() {
        let (project, comp_id) = masked_solid_project(project::MaskMode::Subtract, false);
        let (buf, empty) = rasterize_l1(&project, &comp_id);
        assert!(!empty);
        assert!(buf.get(25, 50).a < 0.01, "subtracted interior must clear");
        assert!(buf.get(75, 50).a > 0.9, "subtracted exterior must stay");
        for p in buf.px.iter().filter(|p| p.a > 0.5) {
            assert!(p.r > 0.9 && p.g > 0.9 && p.b > 0.9, "{p:?}");
        }
        let (project, comp_id) = masked_solid_project(project::MaskMode::Add, true);
        let (buf, _) = rasterize_l1(&project, &comp_id);
        assert!(buf.get(25, 50).a < 0.01, "inverted interior must clear");
        assert!(buf.get(75, 50).a > 0.9, "inverted exterior must stay");
    }

    fn masked_solid_shaped_project(feather: f32, expansion: f32, opacity: f32) -> (Project, String) {
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        let mut layer = project::Layer::solid("l1", "L", Color::WHITE, 100, 100, tc, out);
        let mut mask = project::Mask::with_path(
            "m1",
            "M",
            project::Path::rectangle(10.0, 10.0, 80.0, 80.0),
        );
        mask.feather.set_value(feather);
        mask.expansion.set_value(expansion);
        mask.opacity.set_value(opacity);
        layer.masks.push(mask);
        comp.add_layer(layer).unwrap();
        project.add_composition(comp).unwrap();
        (project, "c".to_string())
    }

    #[test]
    fn masked_text_and_shape_hold_ink() {
        use std::collections::HashMap;
        // Text layer with a wide Add mask: some glyph ink must survive, and
        // surviving ink must keep the fill color (no black artifacts).
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        let mut text = project::Layer::text(
            "l1", "T", "Hello World", "Arial", 48.0, Color::WHITE, tc, out,
        );
        text.masks.push(project::Mask::with_path(
            "m1",
            "M",
            project::Path::rectangle(-1000.0, -1000.0, 2000.0, 2000.0),
        ));
        comp.add_layer(text).unwrap();
        project.add_composition(comp).unwrap();
        let layer = eval_first(&project, "c");
        let assets: HashMap<String, std::sync::Arc<image::RgbaImage>> = HashMap::new();
        let (base_w, base_h) = layer_base_dims(&layer, 1920.0, 1080.0, &assets);
        let (buf, _avg, empty) = rasterize_layer(
            &layer, base_w, base_h, 64, 32, 1920.0, 1080.0, Color::BLACK, None, 0.0, 0,
            false, 5.0, &assets,
        );
        assert!(!empty, "masked text must hold ink");
        let ink = buf.px.iter().filter(|p| p.a > 0.5).count();
        assert!(ink > 20, "masked text keeps glyph ink, got {ink}");
        for p in buf.px.iter().filter(|p| p.a > 0.5) {
            let (r, g, b) = (p.r / p.a, p.g / p.a, p.b / p.a);
            assert!(r > 0.9 && g > 0.9 && b > 0.9, "text ink stays white: {p:?}");
        }
    }

    #[test]
    fn tiny_mask_does_not_mark_layer_empty() {
        // A small but visible mask must still present the layer shell.
        use std::collections::HashMap;
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        let mut layer = project::Layer::solid("l1", "L", Color::WHITE, 1920, 1080, tc, out);
        layer.masks.push(project::Mask::with_path(
            "m1",
            "M",
            project::Path::rectangle(900.0, 500.0, 120.0, 80.0),
        ));
        comp.add_layer(layer).unwrap();
        project.add_composition(comp).unwrap();
        let layer = eval_first(&project, "c");
        let assets: HashMap<String, std::sync::Arc<image::RgbaImage>> = HashMap::new();
        let (buf, _avg, empty) = rasterize_layer(
            &layer, 1920.0, 1080.0, 480, 270, 1920.0, 1080.0, Color::BLACK, None, 0.0, 0,
            false, 5.0, &assets,
        );
        let ink = buf.px.iter().filter(|p| p.a > 0.05).count();
        assert!(ink > 100, "small mask keeps visible ink, got {ink}");
        assert!(!empty, "small mask must not mark the layer empty");
    }

    #[test]
    fn masked_blend_over_backdrop_stays_clean() {
        // Add-masked exotic blend over a solid backdrop buffer.
        use project::BlendMode;
        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc = TimeCode::from_frames(0, 30.0);
        let out = TimeCode::from_frames(150, 30.0);
        let mut top = project::Layer::solid("l1", "L", Color::rgb(1.0, 0.0, 0.0), 100, 100, tc, out);
        top.blend_mode = BlendMode::Multiply;
        top.masks.push(project::Mask::with_path(
            "m1",
            "M",
            project::Path::rectangle(0.0, 0.0, 50.0, 100.0),
        ));
        comp.add_layer(top).unwrap();
        project.add_composition(comp).unwrap();
        let layer = eval_first(&project, "c");
        let assets: HashMap<String, std::sync::Arc<image::RgbaImage>> = HashMap::new();
        let mut bg = FloatBuf::clear(100, 100);
        for p in bg.px.iter_mut() {
            *p = Px::from_color(Color::WHITE);
        }
        let (buf, _avg, empty) = rasterize_layer(
            &layer, 100.0, 100.0, 100, 100, 1920.0, 1080.0, Color::WHITE, Some(&bg), 0.0, 0,
            false, 5.0, &assets,
        );
        assert!(!empty);
        // Masked half: multiply red over white = red, opaque.
        let inside = buf.get(25, 50);
        assert!(inside.a > 0.9 && inside.r > 0.9 && inside.g < 0.1, "{inside:?}");
        // Unmasked half: transparent (shows backdrop through the shell).
        assert!(buf.get(75, 50).a < 0.01, "{:?}", buf.get(75, 50));
    }

    #[test]
    fn masked_solid_feather_expansion_opacity_stay_clean() {
        for (feather, expansion, opacity) in [(8.0, 0.0, 100.0), (0.0, 6.0, 100.0), (0.0, 0.0, 50.0), (6.0, 4.0, 80.0)] {
            let (project, comp_id) = masked_solid_shaped_project(feather, expansion, opacity);
            let (buf, empty) = rasterize_l1(&project, &comp_id);
            assert!(!empty, "shaped mask f={feather} e={expansion} o={opacity} must hold ink");
            // Deep interior survives shaping.
            let center = buf.get(50, 50);
            assert!(center.a > 0.3, "center f={feather} e={expansion} o={opacity}: {center:?}");
            // Far exterior stays clear.
            assert!(buf.get(2, 2).a < 0.01, "corner must stay clear");
            // No black-opaque artifacts anywhere (compare straight colors:
            // premultiplied edge pixels are legitimately gray).
            for p in buf.px.iter().filter(|p| p.a > 0.5) {
                let (r, g, b) = (p.r / p.a, p.g / p.a, p.b / p.a);
                assert!(
                    r > 0.9 && g > 0.9 && b > 0.9,
                    "f={feather} e={expansion} o={opacity}: {p:?}"
                );
            }
        }
    }
}
