use compositor::{EvaluatedEffect, EvaluatedEffectType, EvaluatedLayer};
use image::RgbaImage;
use project::{BlendMode, Color, LayerSource, ShapeType};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use super::affine::{Aff, aff_apply, aff_invert};
use super::buffer::{FloatBuf, blur_buffer};
use super::effects::{RasterFx, apply_effect_pixels};
use super::pixel::Px;
use super::shapes::{fill_ellipse, fill_rect, stroke_path};
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
        LayerSource::Solid { color, .. } => {
            let mut buf = FloatBuf::clear(base_w.ceil().max(1.0) as u32, base_h.ceil().max(1.0) as u32);
            let p = Px::from_color(*color);
            for px in buf.px.iter_mut() {
                *px = p;
            }
            Some(buf)
        }
        LayerSource::Shape { shape_type } => {
            let (w, h) = (base_w.max(2.0), base_h.max(2.0));
            let mut buf = FloatBuf::clear(w.ceil() as u32, h.ceil() as u32);
            match shape_type {
                ShapeType::Rectangle { corner_radius, fill, .. } => {
                    // Corner radius is stored unscaled; content is unscaled.
                    fill_rect(&mut buf, w, h, corner_radius.value, Px::from_color(*fill));
                }
                ShapeType::Ellipse { fill, .. } => {
                    fill_ellipse(&mut buf, w / 2.0, h / 2.0, Px::from_color(*fill));
                }
                ShapeType::Path { path_data, fill } => {
                    stroke_path(&mut buf, path_data, 2.0, Px::from_color(*fill));
                }
            }
            Some(buf)
        }
        LayerSource::Text {
            text,
            font_family,
            font_size,
            fill_color,
            weight,
            italic,
            tracking,
            leading,
            align,
            all_caps,
            stroke_width,
            stroke_color,
            baseline_shift,
            box_width,
        } => {
            // Effective outline: TextOutline effect wins over native stroke.
            let mut sw = stroke_width.value.max(0.0);
            let mut sc = *stroke_color;
            for eff in &layer.effects {
                if !eff.enabled {
                    continue;
                }
                if let EvaluatedEffectType::TextOutline { width, color } = &eff.effect_type {
                    sw = (*width).max(0.0);
                    sc = *color;
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
                weight: *weight,
                italic: *italic,
                tracking: tracking.value,
                leading: leading.value,
                align: *align,
                all_caps: *all_caps,
                stroke_w: sw,
                stroke_col: Px::from_color(sc),
                baseline_shift: baseline_shift.value,
                box_w: box_width.value,
                bevel,
            };
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
) -> u64 {
    let mut h = DefaultHasher::new();
    layer.id.hash(&mut h);
    out_w.hash(&mut h);
    out_h.hash(&mut h);
    playing.hash(&mut h);
    // Time-varying Shader Lab (`time`/`frame` uniforms) invalidates per
    // frame; everything else keys off evaluated values below. This avoids
    // the old `format!("{:?}")` whose AST dumps cost milliseconds per
    // layer per tick during drags.
    let time_varying = layer.effects.iter().any(|e| {
        e.enabled && matches!(&e.effect_type, EvaluatedEffectType::ShaderLab { .. })
    });
    (if time_varying { frame } else { 0 }).hash(&mut h);
    // Evaluated transform (world matrix covers parents).
    let wm = layer.world_matrix();
    for v in [wm.a, wm.b, wm.c, wm.d, wm.tx, wm.ty] {
        v.to_bits().hash(&mut h);
    }
    let t = &layer.transform;
    for v in [t.position.x, t.position.y, t.scale.x, t.scale.y, t.rotation, t.anchor_point.x, t.anchor_point.y] {
        v.to_bits().hash(&mut h);
    }
    layer.effective_opacity.to_bits().hash(&mut h);
    (layer.blend_mode as u8).hash(&mut h);
    layer.is_visible.hash(&mut h);
    // Source content.
    match &layer.source {
        LayerSource::Solid { color, width, height } => {
            0u8.hash(&mut h);
            color_hash(color, &mut h);
            width.hash(&mut h);
            height.hash(&mut h);
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
            weight,
            italic,
            tracking,
            leading,
            align,
            all_caps,
            stroke_width,
            stroke_color,
            baseline_shift,
            box_width,
        } => {
            3u8.hash(&mut h);
            text.value.hash(&mut h);
            font_family.hash(&mut h);
            font_size.value.to_bits().hash(&mut h);
            color_hash(&fill_color.value, &mut h);
            weight.hash(&mut h);
            italic.hash(&mut h);
            tracking.value.to_bits().hash(&mut h);
            leading.value.to_bits().hash(&mut h);
            (*align as u8).hash(&mut h);
            all_caps.hash(&mut h);
            stroke_width.value.to_bits().hash(&mut h);
            color_hash(stroke_color, &mut h);
            baseline_shift.value.to_bits().hash(&mut h);
            box_width.value.to_bits().hash(&mut h);
        }
        LayerSource::Shape { shape_type } => match shape_type {
            ShapeType::Rectangle { width, height, corner_radius, fill } => {
                4u8.hash(&mut h);
                width.value.to_bits().hash(&mut h);
                height.value.to_bits().hash(&mut h);
                corner_radius.value.to_bits().hash(&mut h);
                color_hash(fill, &mut h);
            }
            ShapeType::Ellipse { radius_x, radius_y, fill } => {
                5u8.hash(&mut h);
                radius_x.value.to_bits().hash(&mut h);
                radius_y.value.to_bits().hash(&mut h);
                color_hash(fill, &mut h);
            }
            ShapeType::Path { path_data, fill } => {
                6u8.hash(&mut h);
                path_data.hash(&mut h);
                color_hash(fill, &mut h);
            }
        },
        _ => {
            7u8.hash(&mut h);
        }
    }
    // Effects: identity + on/off + scalar/color params.
    for eff in &layer.effects {
        eff.id.hash(&mut h);
        eff.enabled.hash(&mut h);
        effect_hash(&eff.effect_type, &mut h);
    }
    h.finish()
}

fn color_hash(c: &Color, h: &mut DefaultHasher) {
    c.r.to_bits().hash(h);
    c.g.to_bits().hash(h);
    c.b.to_bits().hash(h);
    c.a.to_bits().hash(h);
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
        EvaluatedEffectType::GradientRamp { color_a, color_b, angle } => {
            color_hash(color_a, h);
            color_hash(color_b, h);
            angle.to_bits().hash(h);
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
    pub png: Arc<Vec<u8>>,
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
    time_s: f32,
    frame: i64,
    playing: bool,
    duration_s: f32,
    assets: &HashMap<String, Arc<RgbaImage>>,
) -> (FloatBuf, Color, bool) {
    let (ow, oh) = (out_w.max(1), out_h.max(1));
    let mut out = FloatBuf::clear(ow, oh);
    if matches!(&layer.source, LayerSource::Adjustment) {
        return (out, backdrop, true);
    }
    let k = ow as f32 / comp_w.max(1.0);
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
    // Local effects.
    let mut work = content;
    apply_layer_fx(&mut work, base_w, base_h, &layer.effects, &fx);
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
        blur_buffer(&mut work, blur_total * k.max(0.25));
    }
    if let Some((intensity, radius)) = bloom {
        apply_bloom(&mut work, intensity, radius * k.max(0.25));
    }
    // World map (local -> canvas px of this AABB box).
    let wm = layer.world_matrix();
    let full = Aff {
        a: wm.a * k,
        b: wm.b * k,
        c: wm.c * k,
        d: wm.d * k,
        tx: (wm.tx + comp_w / 2.0) * k,
        ty: (wm.ty + comp_h / 2.0) * k,
    };
    // AABB origin in canvas px (matches the viewer shell math).
    let bbox = layer.world_bounds(base_w, base_h);
    let ox = (bbox.min.x + comp_w / 2.0) * k;
    let oy = (bbox.min.y + comp_h / 2.0) * k;
    let shifted = Aff {
        a: full.a,
        b: full.b,
        c: full.c,
        d: full.d,
        tx: full.tx - ox.floor(),
        ty: full.ty - oy.floor(),
    };
    // Drop shadow.
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
    let normal = layer.blend_mode == BlendMode::Normal;
    if normal {
        blit_affine(
            &mut out,
            &work,
            shifted,
            layer.effective_opacity.clamp(0.0, 1.0),
            BlendMode::Normal,
            shadow,
        );
    } else {
        // Exotic modes blend against the sampled backdrop average, the
        // same approximation the div compositor used.
        let mut bg = FloatBuf::clear(ow, oh);
        let bp = Px::from_color(backdrop);
        for px in bg.px.iter_mut() {
            *px = bp;
        }
        blit_affine(
            &mut bg,
            &work,
            shifted,
            layer.effective_opacity.clamp(0.0, 1.0),
            BlendMode::Normal,
            shadow,
        );
        for p in bg.px.iter_mut() {
            if p.a <= 0.003 {
                continue;
            }
            let mut d = Px::from_color(backdrop);
            d.blend_over(*p, layer.blend_mode);
            *p = d;
        }
        out = bg;
    }
    let avg = out.average();
    let empty = avg.a < 0.004;
    (out, avg, empty)
}

/// Blit `src` into `dst` through an affine local->dst map with bilinear
/// sampling, opacity, and blend mode. `shadow` draws a blurred offset
/// silhouette underneath first (drop shadow).
pub fn blit_affine(
    dst: &mut FloatBuf,
    src: &FloatBuf,
    map: Aff,
    opacity: f32,
    blend: BlendMode,
    shadow: Option<(f32, f32, f32, Color)>,
) {
    let inv = match aff_invert(map) {
        Some(m) => m,
        None => return,
    };
    let op = opacity.clamp(0.0, 1.0);
    // Drop shadow silhouettes first.
    if let Some((sh_dx, sh_dy, sh_alpha, sh_color)) = shadow {
        if sh_alpha > 0.01 {
            for y in 0..dst.h {
                for x in 0..dst.w {
                    let (u, v) = aff_apply(inv, x as f32 - sh_dx, y as f32 - sh_dy);
                    let s = src.sample(u, v);
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
            // Dissolve dithers per pixel, then draws opaque speckles.
            if blend == BlendMode::Dissolve {
                let h = ((x as u32).wrapping_mul(0x85eb_ca6b) ^ (y as u32).wrapping_mul(0xc2b2_ae35)) % 1000;
                if (h as f32 / 1000.0) > s.a * op {
                    continue;
                }
                if s.a > 1e-6 {
                    let ia = 1.0 / s.a;
                    s.r *= ia;
                    s.g *= ia;
                    s.b *= ia;
                }
                s.a = 1.0;
            } else {
                s.scale(op);
            }
            let dstp = dst.get(x as i32, y as i32);
            let mut out = dstp;
            out.blend_over(s, blend);
            dst.put(x as i32, y as i32, out);
        }
    }
}

/// Base content dims (mirror the viewer estimate for pivot consistency).
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
        LayerSource::Text { text, font_size, .. } => {
            let len = text.value.chars().count().max(1) as f32;
            let fs = font_size.value;
            ((len * fs * 0.6 + 40.0).max(100.0), (fs * 1.4 + 20.0).max(40.0))
        }
        LayerSource::Shape { shape_type } => match shape_type {
            ShapeType::Rectangle { width, height, .. } => (width.value, height.value),
            ShapeType::Ellipse { radius_x, radius_y, .. } => {
                (radius_x.value * 2.0, radius_y.value * 2.0)
            }
            ShapeType::Path { .. } => (400.0, 300.0),
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
pub(crate) fn apply_adjustment(buf: &mut FloatBuf, effects: &[EvaluatedEffect]) {
    let mut blur_total = 0.0f32;
    for eff in effects {
        if !eff.enabled {
            continue;
        }
        if let EvaluatedEffectType::GaussianBlur { radius } = &eff.effect_type {
            blur_total += *radius;
        }
    }
    if blur_total > 0.25 {
        blur_buffer(buf, blur_total);
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
        let k1 = layer_cache_key(&layer, 0, 100, 100, false);
        // Same inputs -> same key (cache hit, no Debug formatting).
        assert_eq!(k1, layer_cache_key(&layer, 0, 100, 100, false));
        // Static layers ignore the frame (cross-frame hits during playback).
        assert_eq!(k1, layer_cache_key(&layer, 99, 100, 100, false));
        // Quality flag participates.
        assert_ne!(k1, layer_cache_key(&layer, 0, 100, 100, true));
        // Size participates.
        assert_ne!(k1, layer_cache_key(&layer, 0, 50, 50, false));
        // Mutating the transform changes the key.
        let mut moved = layer.clone();
        moved.transform.position = project::Vec2::new(10.0, 0.0);
        assert_ne!(k1, layer_cache_key(&moved, 0, 100, 100, false));
    }
}
