use project::TextAlign;
use super::affine::{Aff, aff_mul};
use super::buffer::FloatBuf;
use super::layer::blit_affine;
use super::pixel::{gradient_axis, sample_fill_gradient, GradientAxis, Px};
use project::{BlendMode, FillGradient, Path};
use std::sync::{Mutex, OnceLock};

// Text raster (cosmic-text)
// ---------------------------------------------------------------------------

fn font_system() -> std::sync::MutexGuard<'static, cosmic_text::FontSystem> {
    static FONTS: OnceLock<Mutex<cosmic_text::FontSystem>> = OnceLock::new();
    FONTS
        .get_or_init(|| Mutex::new(cosmic_text::FontSystem::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn swash_cache() -> std::sync::MutexGuard<'static, cosmic_text::SwashCache> {
    static SWASH: OnceLock<Mutex<cosmic_text::SwashCache>> = OnceLock::new();
    SWASH
        .get_or_init(|| Mutex::new(cosmic_text::SwashCache::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[allow(clippy::too_many_arguments)]
pub struct TextSpec<'a> {
    pub text: &'a str,
    pub family: &'a str,
    pub size: f32,
    pub fill: Px,
    /// Linear fill gradient (None = solid `fill`); spans the laid-out
    /// text block.
    pub fill_gradient: Option<FillGradient>,
    pub weight: u16,
    pub italic: bool,
    pub tracking: f32,
    pub leading: f32,
    pub align: TextAlign,
    pub all_caps: bool,
    pub stroke_w: f32,
    pub stroke_col: Px,
    /// Linear stroke gradient (None = solid `stroke_col`).
    pub stroke_gradient: Option<FillGradient>,
    pub baseline_shift: f32,
    pub box_w: f32,
    pub bevel: Option<(f32, f32)>,
}

/// Rasterize text into a transparent pixmap. Returns the pixmap plus the
/// used ink bounds (for centering inside the estimate box).
pub fn raster_text(spec: &TextSpec) -> (FloatBuf, (f32, f32, f32, f32)) {
    use cosmic_text::{Align, Attrs, Family, LetterSpacing, Metrics, Shaping, Style, Weight};
    let content = if spec.all_caps { spec.text.to_uppercase() } else { spec.text.to_string() };
    let size = spec.size.max(4.0);
    let leading = if spec.leading > 0.0 { spec.leading } else { size * 1.2 };
    let wrap_w = if spec.box_w > 0.0 { Some(spec.box_w) } else { None };
    // Wide scratch buffer; cropped to ink afterwards.
    let scratch_w = wrap_w.unwrap_or((content.chars().count().max(1) as f32 * size * 0.75 + 40.0).max(64.0));
    let scratch_h = (leading * (content.lines().count().max(1) as f32 + 1.0)).max(leading * 2.0).max(64.0);
    let mut buf = FloatBuf::clear(scratch_w.ceil() as u32, scratch_h.ceil() as u32);

    let mut fs = font_system();
    let metrics = Metrics::new(size, leading);
    let mut buffer = cosmic_text::Buffer::new(&mut fs, metrics);
    buffer.set_size(wrap_w, Some(scratch_h));
    let mut attrs = Attrs::new();
    attrs.family = Family::Name(spec.family);
    attrs.weight = Weight(spec.weight.clamp(100, 900));
    if spec.italic {
        attrs.style = Style::Italic;
    }
    if spec.tracking.abs() > 0.01 {
        attrs.letter_spacing_opt = Some(LetterSpacing(spec.tracking));
    }
    let align = match spec.align {
        TextAlign::Left | TextAlign::JustifyLeft => Align::Left,
        TextAlign::Center | TextAlign::JustifyCenter => Align::Center,
        TextAlign::Right | TextAlign::JustifyRight => Align::Right,
        TextAlign::JustifyAll => Align::Justified,
    };
    buffer.set_text(&content, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(&mut fs, false);

    let mut swash = swash_cache();
    // Collect glyph draws first (also finds ink bounds).
    let mut glyphs: Vec<GlyphMask> = Vec::new();
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0.0, run.line_y), 1.0);
            let img = match swash.get_image(&mut fs, physical.cache_key) {
                Some(v) => v,
                None => continue,
            };
            use cosmic_text::SwashContent;
            let (mw, mh, data) = match &img.content {
                SwashContent::Mask => (img.placement.width, img.placement.height, img.data.clone()),
                SwashContent::Color => {
                    // Color emoji: blit straight (premultiplied-ish) with fill alpha.
                    blit_color_glyph(&mut buf, physical.x, physical.y, img, spec.fill.a);
                    continue;
                }
                SwashContent::SubpixelMask => {
                    // Treat subpixel coverage as luminance mask.
                    let lum: Vec<u8> = img
                        .data
                        .as_chunks::<4>().0.iter()
                        .map(|p| ((p[0] as u32 + p[1] as u32 + p[2] as u32) / 3) as u8)
                        .collect();
                    (img.placement.width, img.placement.height, lum)
                }
            };
            glyphs.push(GlyphMask {
                // physical.* already include the (0, line_y) offset passed
                // to LayoutGlyph::physical ΓÇö do NOT add line_y again.
                // Placement is y-up relative to the pen: bitmap top sits
                // `top` px ABOVE the pen, so buffer y = pen.y - top.
                x: physical.x + img.placement.left,
                y: physical.y - img.placement.top
                    + spec.baseline_shift.round() as i32,
                w: mw,
                h: mh,
                data,
            });
        }
    }
    drop(swash);

    // Bevel emboss passes (under the fill).
    if let Some((strength, _soft)) = spec.bevel {
        if strength > 0.5 {
            let k = (strength / 100.0 * 0.8).clamp(0.0, 0.9);
            let dark = Px { r: 0.0, g: 0.0, b: 0.0, a: k * 0.9 };
            let light = Px { r: k * 0.9, g: k * 0.9, b: k * 0.9, a: k * 0.9 };
            for g in &glyphs {
                draw_mask(&mut buf, g, -1, -1, dark);
                draw_mask(&mut buf, g, 1, 1, light);
            }
        }
    }
    // Outline ring (8-neighborhood dilate) under the fill.
    let sw_px = spec.stroke_w.round() as i32;
    // Gradient axes span the laid-out block (scratch buffer dims).
    let fill_axis = spec
        .fill_gradient
        .as_ref()
        .map(|g| gradient_axis(buf.w as f32, buf.h as f32, g.angle));
    let stroke_axis = spec
        .stroke_gradient
        .as_ref()
        .map(|g| gradient_axis(buf.w as f32, buf.h as f32, g.angle));
    // Outline: dilate the combined glyph alpha by the stroke width and
    // fill the ring (dilated minus original) under the fill. A true
    // dilation has exact width with no gaps or detachment, unlike stamped
    // offset copies.
    if sw_px >= 1 {
        let white = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let mut alpha = FloatBuf::clear(buf.w, buf.h);
        for g in &glyphs {
            draw_mask(&mut alpha, g, 0, 0, white);
            if spec.weight >= 700 {
                draw_mask(&mut alpha, g, 1, 0, white);
            }
        }
        let flat: Vec<f32> = alpha.px.iter().map(|p| p.a).collect();
        let grown = super::mask::box_extremum(
            &flat,
            buf.w,
            buf.h,
            spec.stroke_w.clamp(1.0, 128.0),
            true,
        );
        for (i, dst) in buf.px.iter_mut().enumerate() {
            let ring = (grown[i] - flat[i]).clamp(0.0, 1.0);
            if ring <= 0.003 {
                continue;
            }
            let (x, y) = ((i as u32 % buf.w) as f32, (i as u32 / buf.w) as f32);
            let p = match (&spec.stroke_gradient, stroke_axis) {
                (Some(grad), Some(axis)) => {
                    let c = sample_fill_gradient(grad, x + 0.5, y + 0.5, axis);
                    Px::from_color_scaled(c, ring)
                }
                _ => {
                    let mut p = spec.stroke_col;
                    p.scale(ring);
                    p
                }
            };
            let mut out = *dst;
            out.over(p);
            *dst = out;
        }
    }
    // Fill + faux-bold second pass.
    for g in &glyphs {
        match (&spec.fill_gradient, fill_axis) {
            (Some(grad), Some(axis)) => draw_mask_gradient(&mut buf, g, 0, 0, grad, axis),
            _ => draw_mask(&mut buf, g, 0, 0, spec.fill),
        }
        if spec.weight >= 700 {
            match (&spec.fill_gradient, fill_axis) {
                (Some(grad), Some(axis)) => draw_mask_gradient(&mut buf, g, 1, 0, grad, axis),
                _ => draw_mask(&mut buf, g, 1, 0, spec.fill),
            }
        }
    }

    // Ink bounds (with baseline shift applied visually below).
    let mut min_x = buf.w as f32;
    let mut min_y = buf.h as f32;
    let mut max_x = 0.0f32;
    let mut max_y = 0.0f32;
    for y in 0..buf.h {
        for x in 0..buf.w {
            if buf.px[(y * buf.w + x) as usize].a > 0.01 {
                min_x = min_x.min(x as f32);
                min_y = min_y.min(y as f32);
                max_x = max_x.max(x as f32 + 1.0);
                max_y = max_y.max(y as f32 + 1.0);
            }
        }
    }
    if max_x <= min_x {
        return (buf, (0.0, 0.0, 0.0, 0.0));
    }
    (buf, (min_x, min_y, max_x, max_y))
}

/// One rasterized glyph mask with its draw origin.
struct GlyphMask {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    data: Vec<u8>,
}

fn draw_mask(buf: &mut FloatBuf, g: &GlyphMask, ox: i32, oy: i32, col: Px) {
    for row in 0..g.h as i32 {
        for column in 0..g.w as i32 {
            let cov = g.data[(row as u32 * g.w + column as u32) as usize] as f32 / 255.0;
            if cov <= 0.0 {
                continue;
            }
            let mut p = col;
            p.scale(cov);
            let dst = buf.get(g.x + ox + column, g.y + oy + row);
            let mut out = dst;
            out.over(p);
            buf.put(g.x + ox + column, g.y + oy + row, out);
        }
    }
}

/// Coverage mask filled from a linear gradient: each pixel samples the
/// gradient at its buffer position, scaled by glyph coverage.
fn draw_mask_gradient(
    buf: &mut FloatBuf,
    g: &GlyphMask,
    ox: i32,
    oy: i32,
    gradient: &FillGradient,
    axis: GradientAxis,
) {
    for row in 0..g.h as i32 {
        for column in 0..g.w as i32 {
            let cov = g.data[(row as u32 * g.w + column as u32) as usize] as f32 / 255.0;
            if cov <= 0.0 {
                continue;
            }
            let (bx, by) = (g.x + ox + column, g.y + oy + row);
            let c = sample_fill_gradient(gradient, bx as f32 + 0.5, by as f32 + 0.5, axis);
            let p = Px::from_color_scaled(c, cov);
            let dst = buf.get(bx, by);
            let mut out = dst;
            out.over(p);
            buf.put(bx, by, out);
        }
    }
}

fn blit_color_glyph(buf: &mut FloatBuf, x: i32, y: i32, img: &cosmic_text::SwashImage, alpha: f32) {
    use cosmic_text::SwashContent;
    if !matches!(img.content, SwashContent::Color) {
        return;
    }
    for row in 0..img.placement.height as i32 {
        for column in 0..img.placement.width as i32 {
            let idx = ((row as u32 * img.placement.width + column as u32) * 4) as usize;
            if idx + 3 >= img.data.len() {
                continue;
            }
            // cosmic color glyphs are RGBA straight.
            let p = Px {
                r: img.data[idx] as f32 / 255.0,
                g: img.data[idx + 1] as f32 / 255.0,
                b: img.data[idx + 2] as f32 / 255.0,
                a: img.data[idx + 3] as f32 / 255.0 * alpha,
            };
            if p.a <= 0.0 {
                continue;
            }
            // Same y-up placement convention as masks.
            let dst = buf.get(x + img.placement.left + column, y - img.placement.top + row);
            let mut out = dst;
            out.over(p);
            buf.put(x + img.placement.left + column, y - img.placement.top + row, out);
        }
    }
}

/// Rasterize text flowing along a path (shared path system → text paths).
/// Each glyph is placed at its pen distance along the path and rotated to
/// the path tangent. Returns the pixmap (with local origin at pixel
/// `(−ox, −oy)`), ink bounds in layer-local coords, and the output origin
/// offset for placement. Falls back to straight layout for degenerate paths.
pub fn raster_text_on_path(
    spec: &TextSpec,
    path: &Path,
) -> (FloatBuf, (f32, f32, f32, f32), (f32, f32)) {
    use cosmic_text::{Align, Attrs, Family, LetterSpacing, Metrics, Shaping, Style, Weight};
    let content = if spec.all_caps { spec.text.to_uppercase() } else { spec.text.to_string() };
    if content.is_empty() {
        return (FloatBuf::clear(8, 8), (0.0, 0.0, 0.0, 0.0), (0.0, 0.0));
    }
    let size = spec.size.max(4.0);
    let leading = if spec.leading > 0.0 { spec.leading } else { size * 1.2 };
    let mut fs = font_system();
    let metrics = Metrics::new(size, leading);
    let mut buffer = cosmic_text::Buffer::new(&mut fs, metrics);
    let mut attrs = Attrs::new();
    attrs.family = Family::Name(spec.family);
    attrs.weight = Weight(spec.weight.clamp(100, 900));
    if spec.italic {
        attrs.style = Style::Italic;
    }
    if spec.tracking.abs() > 0.01 {
        attrs.letter_spacing_opt = Some(LetterSpacing(spec.tracking));
    }
    let align = match spec.align {
        TextAlign::Left | TextAlign::JustifyLeft => Align::Left,
        TextAlign::Center | TextAlign::JustifyCenter => Align::Center,
        TextAlign::Right | TextAlign::JustifyRight => Align::Right,
        TextAlign::JustifyAll => Align::Justified,
    };
    buffer.set_text(&content, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(&mut fs, false);

    // Collect mask glyphs with per-run pen distances.
    struct Placed {
        mask: GlyphMask,
        pen: f32,
    }
    let mut swash = swash_cache();
    let mut placed: Vec<Placed> = Vec::new();
    for run in buffer.layout_runs() {
        let mut run_start: Option<f32> = None;
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0.0, run.line_y), 1.0);
            let img = match swash.get_image(&mut fs, physical.cache_key) {
                Some(v) => v,
                None => continue,
            };
            use cosmic_text::SwashContent;
            let origin_x = run_start.get_or_insert(physical.x as f32);
            let pen = physical.x as f32 - *origin_x;
            match &img.content {
                SwashContent::Mask => {
                    placed.push(Placed {
                        mask: GlyphMask {
                            x: physical.x + img.placement.left,
                            y: physical.y - img.placement.top + spec.baseline_shift.round() as i32,
                            w: img.placement.width,
                            h: img.placement.height,
                            data: img.data.clone(),
                        },
                        pen,
                    });
                }
                SwashContent::Color | SwashContent::SubpixelMask => {
                    // Color/animated glyphs ride the path unrotated (rare).
                    placed.push(Placed {
                        mask: GlyphMask {
                            x: physical.x + img.placement.left,
                            y: physical.y - img.placement.top,
                            w: img.placement.width,
                            h: img.placement.height,
                            data: vec![255u8; (img.placement.width * img.placement.height) as usize],
                        },
                        pen,
                    });
                }
            }
        }
    }
    drop(swash);
    if placed.is_empty() {
        return (FloatBuf::clear(8, 8), (0.0, 0.0, 0.0, 0.0), (0.0, 0.0));
    }
    // Output sized to the path bounds + glyph padding, expanded to include
    // the local origin so pixel (0,0) is layer-local (0,0) — no centering.
    let (pmin, pmax) = path.bounds().unwrap_or((project::Vec2::ZERO, project::Vec2::new(64.0, 64.0)));
    let pad = size * 1.5 + 8.0;
    let (mut ox, mut oy) = (pmin.x - pad, pmin.y - pad);
    let (mut x1, mut y1) = (pmax.x + pad, pmax.y + pad);
    ox = ox.min(0.0);
    oy = oy.min(0.0);
    x1 = x1.max(0.0);
    y1 = y1.max(0.0);
    let (bw, bh) = ((x1 - ox).max(8.0), (y1 - oy).max(8.0));
    let mut buf = FloatBuf::clear(bw.ceil() as u32, bh.ceil() as u32);
    // On-path gradients sample per glyph at its anchor (output space).
    let path_fill_axis = spec
        .fill_gradient
        .as_ref()
        .map(|g| gradient_axis(bw, bh, g.angle));
    let path_stroke_axis = spec
        .stroke_gradient
        .as_ref()
        .map(|g| gradient_axis(bw, bh, g.angle));
    let path_len = path.length(0.5).max(1.0);
    let sw_px = spec.stroke_w.round() as i32;
    for p in &placed {
        // Glyph center rides the path at pen-distance / path-length.
        let ratio = (p.pen / path_len).clamp(0.0, 1.0);
        let anchor = match path.point_at_ratio(ratio, 0.5) {
            Some(v) => v,
            None => continue,
        };
        let angle = path.tangent_at_ratio(ratio, 0.5).unwrap_or(0.0);
        // Compose the glyph (bevel + outline + fill) into a temp buffer.
        // Gradient fills sample once per glyph at its path anchor so the
        // ramp flows along the whole path in output space.
        let (ax, ay) = (anchor.x - ox, anchor.y - oy);
        let glyph_fill = match (&spec.fill_gradient, path_fill_axis) {
            (Some(grad), Some(axis)) => Px::from_color(sample_fill_gradient(grad, ax, ay, axis)),
            _ => spec.fill,
        };
        let glyph_stroke = match (&spec.stroke_gradient, path_stroke_axis) {
            (Some(grad), Some(axis)) => Px::from_color(sample_fill_gradient(grad, ax, ay, axis)),
            _ => spec.stroke_col,
        };
        let (gw, gh) = ((p.mask.w + 8) as i32, (p.mask.h + 8) as i32);
        let mut tmp = FloatBuf::clear(gw as u32, gh as u32);
        let gx = 4i32;
        let gy = 4i32;
        let shifted = GlyphMask { x: gx, y: gy, w: p.mask.w, h: p.mask.h, data: p.mask.data.clone() };
        if let Some((strength, _)) = spec.bevel {
            if strength > 0.5 {
                let k = (strength / 100.0 * 0.8).clamp(0.0, 0.9);
                draw_mask(&mut tmp, &shifted, -1, -1, Px { r: 0.0, g: 0.0, b: 0.0, a: k * 0.9 });
                draw_mask(&mut tmp, &shifted, 1, 1, Px { r: k * 0.9, g: k * 0.9, b: k * 0.9, a: k * 0.9 });
            }
        }
        if sw_px >= 1 {
            // Dilation outline in the temp buffer (same as straight text:
            // exact width, no gaps or detachment).
            let white = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            let mut alpha = FloatBuf::clear(tmp.w, tmp.h);
            draw_mask(&mut alpha, &shifted, 0, 0, white);
            if spec.weight >= 700 {
                draw_mask(&mut alpha, &shifted, 1, 0, white);
            }
            let flat: Vec<f32> = alpha.px.iter().map(|p| p.a).collect();
            let grown = super::mask::box_extremum(
                &flat,
                tmp.w,
                tmp.h,
                spec.stroke_w.clamp(1.0, 128.0),
                true,
            );
            for (i, dst) in tmp.px.iter_mut().enumerate() {
                let ring = (grown[i] - flat[i]).clamp(0.0, 1.0);
                if ring <= 0.003 {
                    continue;
                }
                let mut p = glyph_stroke;
                p.scale(ring);
                let mut out = *dst;
                out.over(p);
                *dst = out;
            }
        }
        draw_mask(&mut tmp, &shifted, 0, 0, glyph_fill);
        if spec.weight >= 700 {
            draw_mask(&mut tmp, &shifted, 1, 0, glyph_fill);
        }
        // Rotate about the glyph center onto the path point.
        let (cx, cy) = (gw as f32 * 0.5, gh as f32 * 0.5);
        let rad = angle.to_radians();
        let (sn, cs) = (rad.sin(), rad.cos());
        let rot = Aff { a: cs, b: sn, c: -sn, d: cs, tx: 0.0, ty: 0.0 };
        let to_dst = Aff { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: anchor.x - ox, ty: anchor.y - oy };
        let from_src = Aff { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: -cx, ty: -cy };
        let map = aff_mul(to_dst, aff_mul(rot, from_src));
        blit_affine(&mut buf, &tmp, map, 1.0, BlendMode::Normal, None, None);
    }
    // Ink bounds.
    let mut min_x = buf.w as f32;
    let mut min_y = buf.h as f32;
    let mut max_x = 0.0f32;
    let mut max_y = 0.0f32;
    for y in 0..buf.h {
        for x in 0..buf.w {
            if buf.px[(y * buf.w + x) as usize].a > 0.01 {
                min_x = min_x.min(x as f32);
                min_y = min_y.min(y as f32);
                max_x = max_x.max(x as f32 + 1.0);
                max_y = max_y.max(y as f32 + 1.0);
            }
        }
    }
    if max_x <= min_x {
        return (buf, (0.0, 0.0, 0.0, 0.0), (ox, oy));
    }
    // Bounds back in layer-local coords (remove the output offset).
    (buf, (min_x + ox, min_y + oy, max_x + ox, max_y + oy), (ox, oy))
}

// ---------------------------------------------------------------------------
