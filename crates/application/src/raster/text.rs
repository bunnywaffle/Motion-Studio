use project::TextAlign;
use super::buffer::FloatBuf;
use super::pixel::Px;
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
    pub weight: u16,
    pub italic: bool,
    pub tracking: f32,
    pub leading: f32,
    pub align: TextAlign,
    pub all_caps: bool,
    pub stroke_w: f32,
    pub stroke_col: Px,
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
        TextAlign::Left => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::Right,
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
                        .chunks_exact(4)
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
    if sw_px >= 1 {
        let ring: [(i32, i32); 8] = [
            (-sw_px, 0),
            (sw_px, 0),
            (0, -sw_px),
            (0, sw_px),
            (-sw_px, -sw_px),
            (sw_px, -sw_px),
            (-sw_px, sw_px),
            (sw_px, sw_px),
        ];
        for g in &glyphs {
            for (ox, oy) in ring {
                draw_mask(&mut buf, g, ox, oy, spec.stroke_col);
            }
        }
    }
    // Fill + faux-bold second pass.
    for g in &glyphs {
        draw_mask(&mut buf, g, 0, 0, spec.fill);
        if spec.weight >= 700 {
            draw_mask(&mut buf, g, 1, 0, spec.fill);
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

// ---------------------------------------------------------------------------
