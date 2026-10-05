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
    /// Stroke placement (parsed from the layer's position/order strings).
    pub stroke_pos: StrokePos,
    /// Fill paints over the stroke (false = stroke paints over the fill).
    pub stroke_fill_over: bool,
    /// Outline offset px: shifts the whole band outward (+) or inward (-).
    pub stroke_offset: f32,
    pub baseline_shift: f32,
    pub box_w: f32,
    pub bevel: Option<(f32, f32)>,
}

/// Which side(s) of the glyph edge the stroke band occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokePos {
    Inside,
    Center,
    Outside,
}

impl StrokePos {
    pub fn from_label(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "inside" => Self::Inside,
            "outside" => Self::Outside,
            _ => Self::Center,
        }
    }
}

/// Resolved stroke extents: outward/inward band radii after width,
/// position, and offset. Center splits the width across the edge (so it
/// visibly grows from the edge); offset shifts the band outward (+).
#[derive(Debug, Clone, Copy)]
pub struct StrokeLayout {
    pub outer: f32,
    pub inner: f32,
    pub fill_over: bool,
}

impl StrokeLayout {
    pub fn new(width: f32, offset: f32, pos: StrokePos, fill_over: bool) -> Self {
        let w = width.max(0.0);
        let (mut outer, mut inner) = match pos {
            StrokePos::Inside => (0.0, w),
            StrokePos::Outside => (w, 0.0),
            StrokePos::Center => (w * 0.5, w * 0.5),
        };
        outer = (outer + offset).max(0.0);
        inner = (inner - offset).max(0.0);
        Self { outer, inner, fill_over }
    }

    /// Any band worth rasterizing (matches the old `round(w) >= 1` gate).
    pub fn active(&self) -> bool {
        self.outer >= 0.5 || self.inner >= 0.5
    }

    /// Max outward/inward reach, for padding.
    pub fn extent(&self) -> f32 {
        self.outer.max(self.inner)
    }
}

/// Paint stroke bands into `dst`: outer = dilated minus flat, inner =
/// flat minus eroded (either side absent when its radius is zero).
/// `col_at` returns the unscaled stroke color at integer pixel coords.
fn paint_stroke_bands(
    dst: &mut FloatBuf,
    flat: &[f32],
    grown: Option<&[f32]>,
    eroded: Option<&[f32]>,
    col_at: impl Fn(f32, f32) -> Px,
) {
    let w = dst.w;
    for (i, dst) in dst.px.iter_mut().enumerate() {
        let mut band = 0.0f32;
        if let Some(g) = grown {
            band += (g[i] - flat[i]).clamp(0.0, 1.0);
        }
        if let Some(e) = eroded {
            band += (flat[i] - e[i]).clamp(0.0, 1.0);
        }
        band = band.clamp(0.0, 1.0);
        if band <= 0.003 {
            continue;
        }
        let (x, y) = ((i as u32 % w) as f32, (i as u32 / w) as f32);
        let mut p = col_at(x, y);
        p.scale(band);
        let mut out = *dst;
        out.over(p);
        *dst = out;
    }
}

/// Rasterize text into a transparent pixmap. Returns the pixmap plus the
/// used ink bounds (for centering inside the estimate box).
pub fn raster_text(spec: &TextSpec) -> (FloatBuf, (f32, f32, f32, f32)) {
    use cosmic_text::{Align, Attrs, Family, LetterSpacing, Metrics, Shaping, Style, Weight};
    let content = if spec.all_caps { spec.text.to_uppercase() } else { spec.text.to_string() };
    let size = spec.size.max(4.0);
    let leading = if spec.leading > 0.0 { spec.leading } else { size * 1.2 };
    let wrap_w = if spec.box_w > 0.0 { Some(spec.box_w) } else { None };
    // Wide scratch buffer; cropped to ink afterwards. Grown on every side
    // by the stroke reach so wide outlines never clip at the buffer edge.
    let layout = StrokeLayout::new(spec.stroke_w, spec.stroke_offset, spec.stroke_pos, spec.stroke_fill_over);
    let spad = layout.extent().ceil().max(0.0);
    let scratch_w = wrap_w.unwrap_or((content.chars().count().max(1) as f32 * size * 0.75 + 40.0).max(64.0)) + spad * 2.0;
    let scratch_h = (leading * (content.lines().count().max(1) as f32 + 1.0)).max(leading * 2.0).max(64.0) + spad * 2.0;
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
                    blit_color_glyph(&mut buf, physical.x + spad as i32, physical.y + spad as i32, img, spec.fill.a);
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
                // to LayoutGlyph::physical — do NOT add line_y again.
                // Placement is y-up relative to the pen: bitmap top sits
                // `top` px ABOVE the pen, so buffer y = pen.y - top.
                // Shifted by the stroke margin (see scratch sizing above).
                x: physical.x + img.placement.left + spad as i32,
                y: physical.y - img.placement.top
                    + spec.baseline_shift.round() as i32
                    + spad as i32,
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
    // Outline bands (8-neighborhood morphology) under or over the fill
    // per paint order: outer = dilated minus original, inner = original
    // minus eroded. A true dilation has exact width with no gaps or
    // detachment, unlike stamped offset copies.
    // Gradient axes span the laid-out block (scratch buffer dims).
    let fill_axis = spec
        .fill_gradient
        .as_ref()
        .map(|g| gradient_axis(buf.w as f32, buf.h as f32, g.angle));
    let stroke_axis = spec
        .stroke_gradient
        .as_ref()
        .map(|g| gradient_axis(buf.w as f32, buf.h as f32, g.angle));
    // Fill + faux-bold pass (order vs stroke bands depends on paint order).
    let draw_fill = |buf: &mut FloatBuf| {
        for g in &glyphs {
            match (&spec.fill_gradient, fill_axis) {
                (Some(grad), Some(axis)) => draw_mask_gradient(buf, g, 0, 0, grad, axis),
                _ => draw_mask(buf, g, 0, 0, spec.fill),
            };
            if spec.weight >= 700 {
                match (&spec.fill_gradient, fill_axis) {
                    (Some(grad), Some(axis)) => draw_mask_gradient(buf, g, 1, 0, grad, axis),
                    _ => draw_mask(buf, g, 1, 0, spec.fill),
                }
            }
        }
    };
    if !layout.fill_over {
        draw_fill(&mut buf);
    }
    if layout.active() {
        let white = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let mut alpha = FloatBuf::clear(buf.w, buf.h);
        for g in &glyphs {
            draw_mask(&mut alpha, g, 0, 0, white);
            if spec.weight >= 700 {
                draw_mask(&mut alpha, g, 1, 0, white);
            }
        }
        let flat: Vec<f32> = alpha.px.iter().map(|p| p.a).collect();
        let grown = (layout.outer >= 0.5).then(|| {
            super::mask::box_extremum(&flat, buf.w, buf.h, layout.outer.clamp(1.0, 128.0), true)
        });
        let eroded = (layout.inner >= 0.5).then(|| {
            super::mask::box_extremum(&flat, buf.w, buf.h, layout.inner.clamp(1.0, 128.0), false)
        });
        paint_stroke_bands(&mut buf, &flat, grown.as_deref(), eroded.as_deref(), |x, y| {
            match (&spec.stroke_gradient, stroke_axis) {
                (Some(grad), Some(axis)) => {
                    Px::from_color(sample_fill_gradient(grad, x + 0.5, y + 0.5, axis))
                }
                _ => spec.stroke_col,
            }
        });
    }
    if layout.fill_over {
        draw_fill(&mut buf);
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
    let layout = StrokeLayout::new(spec.stroke_w, spec.stroke_offset, spec.stroke_pos, spec.stroke_fill_over);
    let pad = size * 1.5 + 8.0 + layout.extent();
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
        // Temp holds the glyph plus the outward band (rotation margin
        // aside, same as before); the inward band lives inside the glyph.
        let spad = layout.outer.ceil().max(0.0) as i32 + 2;
        let spad = spad.max(4);
        let (gw, gh) = (p.mask.w as i32 + spad * 2, p.mask.h as i32 + spad * 2);
        let mut tmp = FloatBuf::clear(gw as u32, gh as u32);
        let gx = spad;
        let gy = spad;
        let shifted = GlyphMask { x: gx, y: gy, w: p.mask.w, h: p.mask.h, data: p.mask.data.clone() };
        if let Some((strength, _)) = spec.bevel {
            if strength > 0.5 {
                let k = (strength / 100.0 * 0.8).clamp(0.0, 0.9);
                draw_mask(&mut tmp, &shifted, -1, -1, Px { r: 0.0, g: 0.0, b: 0.0, a: k * 0.9 });
                draw_mask(&mut tmp, &shifted, 1, 1, Px { r: k * 0.9, g: k * 0.9, b: k * 0.9, a: k * 0.9 });
            }
        }
        if layout.active() {
            // Dilation outline in the temp buffer (same as straight text:
            // exact width, no gaps or detachment).
            let white = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            let mut alpha = FloatBuf::clear(tmp.w, tmp.h);
            draw_mask(&mut alpha, &shifted, 0, 0, white);
            if spec.weight >= 700 {
                draw_mask(&mut alpha, &shifted, 1, 0, white);
            }
            let flat: Vec<f32> = alpha.px.iter().map(|p| p.a).collect();
            let grown = (layout.outer >= 0.5).then(|| {
                super::mask::box_extremum(&flat, tmp.w, tmp.h, layout.outer.clamp(1.0, 128.0), true)
            });
            let eroded = (layout.inner >= 0.5).then(|| {
                super::mask::box_extremum(&flat, tmp.w, tmp.h, layout.inner.clamp(1.0, 128.0), false)
            });
            if !layout.fill_over {
                draw_mask(&mut tmp, &shifted, 0, 0, glyph_fill);
                if spec.weight >= 700 {
                    draw_mask(&mut tmp, &shifted, 1, 0, glyph_fill);
                }
            }
            paint_stroke_bands(&mut tmp, &flat, grown.as_deref(), eroded.as_deref(), |_, _| glyph_stroke);
            if layout.fill_over {
                draw_mask(&mut tmp, &shifted, 0, 0, glyph_fill);
                if spec.weight >= 700 {
                    draw_mask(&mut tmp, &shifted, 1, 0, glyph_fill);
                }
            }
        } else {
            draw_mask(&mut tmp, &shifted, 0, 0, glyph_fill);
            if spec.weight >= 700 {
                draw_mask(&mut tmp, &shifted, 1, 0, glyph_fill);
            }
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
// Text Split Animator (2D)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TextSplitParams {
    pub split_by: project::TextSplitBy,
    pub order: project::TextSplitOrder,
    pub random_seed: i32,
    pub progress: f32, // 0.0 .. 100.0%
    pub spread: f32,   // 0.0 .. 100.0%
    /// Keep every token at its layout slot while progress changes (no
    /// reflow jumps); false recenters the visible tokens each frame.
    pub lock_layout: bool,
    pub easing: project::TextSplitEasing,
    pub offset_position: project::Vec2,
    pub offset_rotation: f32,
    pub offset_opacity: f32,
    pub anchor_alignment: project::Vec2,
}

struct TokenGlyph {
    mask: GlyphMask,
    rel_x: f32,
    rel_y: f32,
}

struct TokenItem {
    glyphs: Vec<TokenGlyph>,
    bounds: (f32, f32, f32, f32), // min_x, min_y, max_x, max_y
    center: (f32, f32),
}

/// Rasterize text with per-character or per-word 2D split animation transforms.
pub fn raster_text_split(
    spec: &TextSpec,
    params: &TextSplitParams,
) -> (FloatBuf, (f32, f32, f32, f32)) {
    use cosmic_text::{Align, Attrs, Family, LetterSpacing, Metrics, Shaping, Style, Weight};
    let content = if spec.all_caps { spec.text.to_uppercase() } else { spec.text.to_string() };
    if content.is_empty() {
        return (FloatBuf::clear(8, 8), (0.0, 0.0, 0.0, 0.0));
    }
    let size = spec.size.max(4.0);
    let leading = if spec.leading > 0.0 { spec.leading } else { size * 1.2 };
    let wrap_w = if spec.box_w > 0.0 { Some(spec.box_w) } else { None };
    let _scratch_w = wrap_w.unwrap_or((content.chars().count().max(1) as f32 * size * 0.75 + 120.0).max(64.0));
    let scratch_h = (leading * (content.lines().count().max(1) as f32 + 2.0)).max(leading * 3.0).max(64.0);

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

    struct RawGlyph {
        mask: GlyphMask,
        byte_start: usize,
        byte_end: usize,
    }

    let mut swash = swash_cache();
    let mut raw_glyphs: Vec<RawGlyph> = Vec::new();
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
                SwashContent::Color | SwashContent::SubpixelMask => {
                    (img.placement.width, img.placement.height, vec![255u8; (img.placement.width * img.placement.height) as usize])
                }
            };
            raw_glyphs.push(RawGlyph {
                mask: GlyphMask {
                    x: physical.x + img.placement.left,
                    y: physical.y - img.placement.top + spec.baseline_shift.round() as i32,
                    w: mw,
                    h: mh,
                    data,
                },
                byte_start: glyph.start,
                byte_end: glyph.end,
            });
        }
    }
    drop(swash);

    if raw_glyphs.is_empty() {
        return (FloatBuf::clear(8, 8), (0.0, 0.0, 0.0, 0.0));
    }

    // Group raw glyphs into tokens (either individual glyphs or words)
    let mut tokens: Vec<TokenItem> = Vec::new();
    match params.split_by {
        project::TextSplitBy::Character => {
            for rg in raw_glyphs {
                let gx = rg.mask.x as f32;
                let gy = rg.mask.y as f32;
                let gw = rg.mask.w as f32;
                let gh = rg.mask.h as f32;
                let cx = gx + gw * 0.5;
                let cy = gy + gh * 0.5;
                tokens.push(TokenItem {
                    glyphs: vec![TokenGlyph {
                        mask: rg.mask,
                        rel_x: 0.0,
                        rel_y: 0.0,
                    }],
                    bounds: (gx, gy, gx + gw, gy + gh),
                    center: (cx, cy),
                });
            }
        }
        project::TextSplitBy::Word => {
            // Group by word spans in the underlying text
            let mut current_glyphs: Vec<RawGlyph> = Vec::new();
            for rg in raw_glyphs {
                let slice = content.get(rg.byte_start..rg.byte_end).unwrap_or("");
                let is_whitespace = slice.chars().all(|c| c.is_whitespace());
                if is_whitespace && !current_glyphs.is_empty() {
                    // Flush current word
                    tokens.push(make_token(std::mem::take(&mut current_glyphs)));
                } else if !is_whitespace {
                    current_glyphs.push(rg);
                }
            }
            if !current_glyphs.is_empty() {
                tokens.push(make_token(current_glyphs));
            }
        }
    }

    fn make_token(glyphs: Vec<RawGlyph>) -> TokenItem {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for g in &glyphs {
            let gx = g.mask.x as f32;
            let gy = g.mask.y as f32;
            let gw = g.mask.w as f32;
            let gh = g.mask.h as f32;
            min_x = min_x.min(gx);
            min_y = min_y.min(gy);
            max_x = max_x.max(gx + gw);
            max_y = max_y.max(gy + gh);
        }
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let token_glyphs = glyphs
            .into_iter()
            .map(|g| TokenGlyph {
                rel_x: g.mask.x as f32 - min_x,
                rel_y: g.mask.y as f32 - min_y,
                mask: g.mask,
            })
            .collect();
        TokenItem {
            glyphs: token_glyphs,
            bounds: (min_x, min_y, max_x, max_y),
            center: (cx, cy),
        }
    }

    if tokens.is_empty() {
        return (FloatBuf::clear(8, 8), (0.0, 0.0, 0.0, 0.0));
    }

    // Determine normalized rank u_i in [0.0, 1.0] for each token
    let n = tokens.len();
    let mut ranks: Vec<f32> = Vec::with_capacity(n);
    match params.order {
        project::TextSplitOrder::FromStart => {
            for i in 0..n {
                let u = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
                ranks.push(u);
            }
        }
        project::TextSplitOrder::FromEnd => {
            for i in 0..n {
                let u = if n > 1 { (n - 1 - i) as f32 / (n - 1) as f32 } else { 0.0 };
                ranks.push(u);
            }
        }
        project::TextSplitOrder::Random => {
            // Seeded permutation using LCG
            let mut perm: Vec<usize> = (0..n).collect();
            let mut state = (params.random_seed as u64).wrapping_add(1);
            for i in (1..n).rev() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let j = (state >> 33) as usize % (i + 1);
                perm.swap(i, j);
            }
            ranks.resize(n, 0.0);
            for (rank_idx, &orig_idx) in perm.iter().enumerate() {
                ranks[orig_idx] = if n > 1 { rank_idx as f32 / (n - 1) as f32 } else { 0.0 };
            }
        }
    }

    let progress_norm = (params.progress / 100.0).clamp(0.0, 1.0);
    let spread_norm = (params.spread / 100.0).clamp(0.01, 1.0);

    // Bounding bounds of all tokens combined with padding
    let mut total_min_x = f32::INFINITY;
    let mut total_min_y = f32::INFINITY;
    let mut total_max_x = f32::NEG_INFINITY;
    let mut total_max_y = f32::NEG_INFINITY;
    for t in &tokens {
        total_min_x = total_min_x.min(t.bounds.0);
        total_min_y = total_min_y.min(t.bounds.1);
        total_max_x = total_max_x.max(t.bounds.2);
        total_max_y = total_max_y.max(t.bounds.3);
    }

    let layout_split = StrokeLayout::new(spec.stroke_w, spec.stroke_offset, spec.stroke_pos, spec.stroke_fill_over);
    let pad_w = (params.offset_position.x.abs() + size * 2.0 + 80.0 + layout_split.extent()).max(64.0);
    let pad_h = (params.offset_position.y.abs() + size * 2.0 + 80.0 + layout_split.extent()).max(64.0);
    let out_ox = total_min_x - pad_w;
    let out_oy = total_min_y - pad_h;
    let out_w = ((total_max_x - total_min_x) + pad_w * 2.0).ceil().max(32.0) as u32;
    let out_h = ((total_max_y - total_min_y) + pad_h * 2.0).ceil().max(32.0) as u32;

    let mut buf = FloatBuf::clear(out_w, out_h);

    let fill_axis = spec.fill_gradient.as_ref().map(|g| gradient_axis(out_w as f32, out_h as f32, g.angle));
    let stroke_axis = spec.stroke_gradient.as_ref().map(|g| gradient_axis(out_w as f32, out_h as f32, g.angle));

    for (i, token) in tokens.iter().enumerate() {
        let u = ranks[i];
        // Activation calculation with spread/overlap
        let raw_w = ((progress_norm - u * (1.0 - spread_norm)) / spread_norm).clamp(0.0, 1.0);
        let weight = params.easing.apply(raw_w);
        let offset_factor = 1.0 - weight;

        let delta_pos = params.offset_position * offset_factor;
        let delta_rot = params.offset_rotation * offset_factor;
        let min_opacity = (params.offset_opacity / 100.0).clamp(0.0, 1.0);
        let alpha_factor = (1.0 - (1.0 - min_opacity) * offset_factor).clamp(0.0, 1.0);

        if alpha_factor <= 0.003 {
            continue;
        }

        let tw = (token.bounds.2 - token.bounds.0).max(1.0);
        let th = (token.bounds.3 - token.bounds.1).max(1.0);

        // Local token anchor point (relative to token bounds min)
        // User anchor_alignment is -1.0..1.0 or pixel offset; normalize around center
        let anc_x = tw * 0.5 + params.anchor_alignment.x;
        let anc_y = th * 0.5 + params.anchor_alignment.y;

        // Render token into a temporary sub-buffer (padded for the
        // outward band; the inward band lives inside the glyphs).
        let tmp_pad = (layout_split.outer.ceil() as i32 + 4).max(8) as f32;
        let tmp_w = (tw + tmp_pad * 2.0).ceil() as u32;
        let tmp_h = (th + tmp_pad * 2.0).ceil() as u32;
        let mut tmp = FloatBuf::clear(tmp_w, tmp_h);

        // Draw glyphs into temp buffer
        for g in &token.glyphs {
            let gx = (g.rel_x + tmp_pad).round() as i32;
            let gy = (g.rel_y + tmp_pad).round() as i32;
            let shifted = GlyphMask {
                x: gx,
                y: gy,
                w: g.mask.w,
                h: g.mask.h,
                data: g.mask.data.clone(),
            };

            if let Some((strength, _)) = spec.bevel {
                if strength > 0.5 {
                    let k = (strength / 100.0 * 0.8).clamp(0.0, 0.9);
                    draw_mask(&mut tmp, &shifted, -1, -1, Px { r: 0.0, g: 0.0, b: 0.0, a: k * 0.9 });
                    draw_mask(&mut tmp, &shifted, 1, 1, Px { r: k * 0.9, g: k * 0.9, b: k * 0.9, a: k * 0.9 });
                }
            }

            if layout_split.active() {
                let fill_col = match (&spec.fill_gradient, fill_axis) {
                    (Some(grad), Some(axis)) => Px::from_color(sample_fill_gradient(grad, token.center.0 - out_ox, token.center.1 - out_oy, axis)),
                    _ => spec.fill,
                };
                let white = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
                let mut a_buf = FloatBuf::clear(tmp.w, tmp.h);
                draw_mask(&mut a_buf, &shifted, 0, 0, white);
                if spec.weight >= 700 {
                    draw_mask(&mut a_buf, &shifted, 1, 0, white);
                }
                let flat: Vec<f32> = a_buf.px.iter().map(|p| p.a).collect();
                let grown = (layout_split.outer >= 0.5).then(|| {
                    super::mask::box_extremum(&flat, tmp.w, tmp.h, layout_split.outer.clamp(1.0, 128.0), true)
                });
                let eroded = (layout_split.inner >= 0.5).then(|| {
                    super::mask::box_extremum(&flat, tmp.w, tmp.h, layout_split.inner.clamp(1.0, 128.0), false)
                });
                if !layout_split.fill_over {
                    draw_mask(&mut tmp, &shifted, 0, 0, fill_col);
                    if spec.weight >= 700 {
                        draw_mask(&mut tmp, &shifted, 1, 0, fill_col);
                    }
                }
                let tcol = match (&spec.stroke_gradient, stroke_axis) {
                    (Some(grad), Some(axis)) => Px::from_color(sample_fill_gradient(grad, token.center.0 - out_ox, token.center.1 - out_oy, axis)),
                    _ => spec.stroke_col,
                };
                paint_stroke_bands(&mut tmp, &flat, grown.as_deref(), eroded.as_deref(), |_, _| tcol);
                if layout_split.fill_over {
                    draw_mask(&mut tmp, &shifted, 0, 0, fill_col);
                    if spec.weight >= 700 {
                        draw_mask(&mut tmp, &shifted, 1, 0, fill_col);
                    }
                }
            } else {
                let fill_col = match (&spec.fill_gradient, fill_axis) {
                    (Some(grad), Some(axis)) => Px::from_color(sample_fill_gradient(grad, token.center.0 - out_ox, token.center.1 - out_oy, axis)),
                    _ => spec.fill,
                };
                draw_mask(&mut tmp, &shifted, 0, 0, fill_col);
                if spec.weight >= 700 {
                    draw_mask(&mut tmp, &shifted, 1, 0, fill_col);
                }
            }
        }

        // Blit token via 2D affine transform into destination buffer
        // Destination pivot point:
        let dst_pivot_x = (token.bounds.0 - out_ox) + anc_x + delta_pos.x;
        let dst_pivot_y = (token.bounds.1 - out_oy) + anc_y + delta_pos.y;
        let src_pivot_x = tmp_pad + anc_x;
        let src_pivot_y = tmp_pad + anc_y;

        let rad = delta_rot.to_radians();
        let (sn, cs) = (rad.sin(), rad.cos());
        let rot = Aff { a: cs, b: sn, c: -sn, d: cs, tx: 0.0, ty: 0.0 };
        let to_dst = Aff { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: dst_pivot_x, ty: dst_pivot_y };
        let from_src = Aff { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: -src_pivot_x, ty: -src_pivot_y };
        let map = aff_mul(to_dst, aff_mul(rot, from_src));

        blit_affine(&mut buf, &tmp, map, alpha_factor, BlendMode::Normal, None, None);
    }

    // Compute ink bounds
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
    // Locked layout anchors on the full token union (progress-independent),
    // so visible characters never jump as progress changes; unlocked keeps
    // the old recenter-on-visible-ink behavior.
    if params.lock_layout {
        (
            buf,
            (
                total_min_x - out_ox,
                total_min_y - out_oy,
                total_max_x - out_ox,
                total_max_y - out_oy,
            ),
        )
    } else {
        (buf, (min_x, min_y, max_x, max_y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use project::{TextSplitBy, TextSplitEasing, TextSplitOrder, Vec2};

    #[test]
    fn test_raster_text_split_smoke() {
        let spec = TextSpec {
            text: "HELLO WORLD",
            family: "Arial",
            size: 32.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            fill_gradient: None,
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 40.0,
            align: TextAlign::Left,
            all_caps: false,
            stroke_w: 0.0,
            stroke_col: Px { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            stroke_gradient: None,
            stroke_pos: StrokePos::Outside,
            stroke_fill_over: true,
            stroke_offset: 0.0,
            baseline_shift: 0.0,
            box_w: 400.0,
            bevel: None,
        };
        let params = TextSplitParams {
            split_by: TextSplitBy::Character,
            order: TextSplitOrder::FromStart,
            random_seed: 12487,
            progress: 50.0,
            spread: 40.0,
            lock_layout: true,
            easing: TextSplitEasing::EaseInOut,
            offset_position: Vec2::new(0.0, -50.0),
            offset_rotation: -25.0,
            offset_opacity: 0.0,
            anchor_alignment: Vec2::new(0.0, 0.0),
        };
        let (buf, bounds) = raster_text_split(&spec, &params);
        assert!(buf.w > 0 && buf.h > 0);
        assert!(bounds.2 >= bounds.0);
    }

    #[test]
    fn test_text_split_lock_keeps_anchor_stable_across_progress() {
        let spec = TextSpec {
            text: "HELLO WORLD",
            family: "Arial",
            size: 32.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            fill_gradient: None,
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 40.0,
            align: TextAlign::Left,
            all_caps: false,
            stroke_w: 0.0,
            stroke_col: Px { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
            stroke_gradient: None,
            stroke_pos: StrokePos::Outside,
            stroke_fill_over: true,
            stroke_offset: 0.0,
            baseline_shift: 0.0,
            box_w: 400.0,
            bevel: None,
        };
        let base = TextSplitParams {
            split_by: TextSplitBy::Character,
            order: TextSplitOrder::FromStart,
            random_seed: 12487,
            progress: 30.0,
            spread: 40.0,
            lock_layout: true,
            easing: TextSplitEasing::EaseInOut,
            offset_position: Vec2::new(0.0, -50.0),
            offset_rotation: -25.0,
            offset_opacity: 0.0,
            anchor_alignment: Vec2::new(0.0, 0.0),
        };
        let (_, locked_lo) = raster_text_split(&spec, &base);
        let mut hi = base.clone();
        hi.progress = 100.0;
        let (_, locked_hi) = raster_text_split(&spec, &hi);
        // Same anchor box at any progress: characters stay put.
        assert_eq!(locked_lo, locked_hi);
        // Unlocked recenters on visible ink: the partial-progress box is
        // narrower than the full-progress box (visible tokens reflow).
        let mut unlocked_lo = base.clone();
        unlocked_lo.lock_layout = false;
        let (_, ink_lo) = raster_text_split(&spec, &unlocked_lo);
        let mut unlocked_hi = hi.clone();
        unlocked_hi.lock_layout = false;
        let (_, ink_hi) = raster_text_split(&spec, &unlocked_hi);
        assert!((ink_lo.2 - ink_lo.0) < (ink_hi.2 - ink_hi.0));
    }

    fn stroke_spec(pos: StrokePos, fill_over: bool, offset: f32) -> TextSpec<'static> {
        TextSpec {
            text: "H",
            family: "Arial",
            size: 48.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            fill_gradient: None,
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 0.0,
            align: TextAlign::Left,
            all_caps: false,
            stroke_w: 10.0,
            stroke_col: Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
            stroke_gradient: None,
            stroke_pos: pos,
            stroke_fill_over: fill_over,
            stroke_offset: offset,
            baseline_shift: 0.0,
            box_w: 0.0,
            bevel: None,
        }
    }

    fn ink_width(spec: &TextSpec) -> f32 {
        let (_, (x0, _, x1, _)) = raster_text(spec);
        x1 - x0
    }

    #[test]
    fn test_stroke_position_controls_ink_growth() {
        // Fill-only baseline (no stroke).
        let mut plain = stroke_spec(StrokePos::Outside, true, 0.0);
        plain.stroke_w = 0.0;
        let base = ink_width(&plain);
        // Outside grows the ink by the full width on each side.
        let outside = ink_width(&stroke_spec(StrokePos::Outside, true, 0.0));
        assert!((outside - base - 20.0).abs() < 3.0, "outside grows +10/side: {outside} vs {base}");
        // Center grows half the width per side (grows from the edge).
        let center = ink_width(&stroke_spec(StrokePos::Center, false, 0.0));
        assert!((center - base - 10.0).abs() < 3.0, "center grows +5/side: {center} vs {base}");
        // Inside never grows past the fill ink.
        let inside = ink_width(&stroke_spec(StrokePos::Inside, false, 0.0));
        assert!((inside - base).abs() < 2.0, "inside stays inside: {inside} vs {base}");
        // Offset pushes the whole band outward.
        let pushed = ink_width(&stroke_spec(StrokePos::Outside, true, 6.0));
        assert!((pushed - base - 32.0).abs() < 3.0, "offset +6 grows +16/side: {pushed} vs {base}");
    }

    #[test]
    fn test_stroke_inside_paints_over_fill() {
        // Inside + stroke-over-fill: pixels just inside the glyph edge are
        // stroke red, and nothing spills outside the fill ink.
        let spec = stroke_spec(StrokePos::Inside, false, 0.0);
        let (buf, (x0, _, x1, _)) = raster_text(&spec);
        let w = buf.w as usize;
        let at = |x: i32, y: i32| buf.px[(y as usize) * w + x as usize];
        // Left edge band must be red-dominant (stroke), not white fill.
        // (The exact outermost column is an AA fringe; scan a few in.)
        let mut red_edge = 0;
        for y in 0..buf.h as i32 {
            for x in x0 as i32..(x0 as i32 + 4) {
                let p = at(x, y);
                if p.a > 0.1 && p.r > 0.5 && p.g < 0.5 {
                    red_edge += 1;
                    break;
                }
            }
        }
        assert!(red_edge > 5, "inside stroke must edge the fill in red, got {red_edge}");
        // Fill-only ink must be at least as wide (no outward spill).
        // (Absolute origins differ by the scratch margin; compare widths.)
        let mut plain = stroke_spec(StrokePos::Outside, true, 0.0);
        plain.stroke_w = 0.0;
        let (_, (px0, _, px1, _)) = raster_text(&plain);
        assert!(
            ((x1 - x0) - (px1 - px0)).abs() < 2.0,
            "inside must not spill out: {x0},{x1} vs {px0},{px1}"
        );
    }
}
