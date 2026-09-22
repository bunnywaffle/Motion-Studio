//! CPU raster viewport compositor: true-color per-pixel previews.
//!
//! gpui renders SVG only as monochrome alpha masks and offers no Div
//! rotation, so rotated or spatially-filtered content cannot preview
//! through divs. This module rasterizes each layer to an AABB-sized RGBA
//! pixmap at canvas scale instead:
//!
//! - base content: solids, shapes (rect/ellipse/pen polyline), cosmic-text
//!   glyphs (weight, italic, tracking, leading, align, caps, stroke,
//!   baseline, box wrap, bevel emboss), decoded images, video slate;
//! - per-pixel effects with real uv/position: checkerboard, gradient,
//!   tiler, warp, perspective, noise, blur, bloom, drop shadow, plus the
//!   whole non-spatial stack through `process_color` (tint, exposure,
//!   vibrance, ...);
//! - Shader Lab runs per-pixel through the CPU interpreter when paused,
//!   and falls back to a cheap probe wash during playback;
//! - one affine rotate/scale/translate blit with bilinear sampling,
//!   opacity, blend modes, then PNG encode for `gpui::img`.
//!
//! Adjustment layers post-process the composite beneath them, matching
//! the fold-in semantics used elsewhere.

use compositor::{EvaluatedEffect, EvaluatedEffectType, EvaluatedLayer};
use image::RgbaImage;
use project::{BlendMode, Color, LayerSource, ShapeType, TextAlign};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Pixels
// ---------------------------------------------------------------------------

/// Straight-alpha linear pixel on the way in, PREMULTIPLIED rgb + alpha
/// once stored in buffers (`r/g/b` already scaled by `a`). All buffer
/// math below preserves that invariant; conversions happen at the edges.
#[derive(Clone, Copy, Debug, Default)]
pub struct Px {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Px {
    pub fn clear() -> Self {
        Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
    }

    /// Straight color -> premultiplied pixel.
    pub fn from_color(c: Color) -> Self {
        Self { r: c.r * c.a, g: c.g * c.a, b: c.b * c.a, a: c.a }
    }

    /// Straight color with explicit alpha scale -> premultiplied pixel.
    pub fn from_color_scaled(c: Color, a_scale: f32) -> Self {
        let a = (c.a * a_scale).clamp(0.0, 1.0);
        Self { r: c.r * a, g: c.g * a, b: c.b * a, a }
    }

    /// Premultiplied pixel -> straight color.
    pub fn to_color(self) -> Color {
        if self.a <= 1e-6 {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        Color::rgba(
            (self.r / self.a).clamp(0.0, 1.0),
            (self.g / self.a).clamp(0.0, 1.0),
            (self.b / self.a).clamp(0.0, 1.0),
            self.a.clamp(0.0, 1.0),
        )
    }

    /// Straight-alpha "over" on premultiplied buffers.
    pub fn over(&mut self, src: Px) {
        let ia = 1.0 - src.a;
        self.r = src.r + self.r * ia;
        self.g = src.g + self.g * ia;
        self.b = src.b + self.b * ia;
        self.a = src.a + self.a * ia;
    }

    /// Scale a premultiplied pixel (opacity fades).
    pub fn scale(&mut self, k: f32) {
        self.r *= k;
        self.g *= k;
        self.b *= k;
        self.a *= k;
    }

    /// Blend-mode composite of `src` over `self` (both premultiplied).
    pub fn blend_over(&mut self, src: Px, mode: BlendMode) {
        if src.a <= 0.0 {
            return;
        }
        let da = self.a;
        let dst_c = if da > 1e-6 {
            [self.r / da, self.g / da, self.b / da]
        } else {
            [0.0, 0.0, 0.0]
        };
        let sa = src.a;
        let src_c = [src.r / sa, src.g / sa, src.b / sa];
        let bc = blend_color(mode, dst_c, src_c);
        // Composite the blended color with src alpha, premultiplied out.
        let ia = 1.0 - sa;
        self.r = bc[0] * sa + self.r * ia;
        self.g = bc[1] * sa + self.g * ia;
        self.b = bc[2] * sa + self.b * ia;
        self.a = sa + da * ia;
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.0;
    if (mx - mn).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if (mx - r).abs() < 1e-6 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (mx - g).abs() < 1e-6 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h.fract(), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s.abs() < 1e-6 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hk = h.fract();
    let tc = |t: f32| {
        let t = t.fract();
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [tc(hk + 1.0 / 3.0), tc(hk), tc(hk - 1.0 / 3.0)]
}

fn blend_color(mode: BlendMode, dst: [f32; 3], src: [f32; 3]) -> [f32; 3] {
    // HSL-space modes work on whole triplets.
    match mode {
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => {
            return blend_color_hsl(mode, dst, src);
        }
        BlendMode::Normal | BlendMode::Dissolve => {
            return [src[0].clamp(0.0, 1.0), src[1].clamp(0.0, 1.0), src[2].clamp(0.0, 1.0)];
        }
        _ => {}
    }
    let ch = |d: f32, s: f32| -> f32 {
        match mode {
            BlendMode::Multiply => d * s,
            BlendMode::Screen => 1.0 - (1.0 - d) * (1.0 - s),
            BlendMode::Overlay => {
                if d < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) }
            }
            BlendMode::Darken => d.min(s),
            BlendMode::Lighten => d.max(s),
            BlendMode::ColorDodge => {
                if s >= 1.0 { 1.0 } else { (d / (1.0 - s)).min(1.0) }
            }
            BlendMode::ColorBurn => {
                if s <= 0.0 { 0.0 } else { (1.0 - (1.0 - d) / s).max(0.0) }
            }
            BlendMode::HardLight => {
                if s < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) }
            }
            BlendMode::SoftLight => (1.0 - 2.0 * s) * d * d + 2.0 * s * d,
            BlendMode::Difference => (d - s).abs(),
            BlendMode::Exclusion => d + s - 2.0 * d * s,
            BlendMode::Add => d + s,
            BlendMode::Subtract => d - s,
            _ => s,
        }
    };
    [
        ch(dst[0], src[0]).clamp(0.0, 1.0),
        ch(dst[1], src[1]).clamp(0.0, 1.0),
        ch(dst[2], src[2]).clamp(0.0, 1.0),
    ]
}

fn blend_color_hsl(mode: BlendMode, dst: [f32; 3], src: [f32; 3]) -> [f32; 3] {
    let (hd, sd, ld) = rgb_to_hsl(dst[0], dst[1], dst[2]);
    let (hs, ss, ls) = rgb_to_hsl(src[0], src[1], src[2]);
    let r = match mode {
        BlendMode::Hue => hsl_to_rgb(hs, sd, ld),
        BlendMode::Saturation => hsl_to_rgb(hd, ss, ld),
        BlendMode::Color => hsl_to_rgb(hs, ss, ld),
        BlendMode::Luminosity => hsl_to_rgb(hd, sd, ls),
        _ => src,
    };
    [r[0].clamp(0.0, 1.0), r[1].clamp(0.0, 1.0), r[2].clamp(0.0, 1.0)]
}

// ---------------------------------------------------------------------------
// Affine helpers (local <-> composition px)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Aff {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

pub fn aff_invert(m: Aff) -> Option<Aff> {
    let det = m.a * m.d - m.b * m.c;
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    Some(Aff {
        a: m.d * inv,
        b: -m.b * inv,
        c: -m.c * inv,
        d: m.a * inv,
        tx: (m.c * m.ty - m.d * m.tx) * inv,
        ty: (m.b * m.tx - m.a * m.ty) * inv,
    })
}

pub fn aff_apply(m: Aff, x: f32, y: f32) -> (f32, f32) {
    (m.a * x + m.c * y + m.tx, m.b * x + m.d * y + m.ty)
}

/// Compose two affines: apply `inner` first, then `outer`.
pub fn aff_mul(outer: Aff, inner: Aff) -> Aff {
    Aff {
        a: outer.a * inner.a + outer.c * inner.b,
        b: outer.b * inner.a + outer.d * inner.b,
        c: outer.a * inner.c + outer.c * inner.d,
        d: outer.b * inner.c + outer.d * inner.d,
        tx: outer.a * inner.tx + outer.c * inner.ty + outer.tx,
        ty: outer.b * inner.tx + outer.d * inner.ty + outer.ty,
    }
}

/// Skew about a pivot (degrees), for the Perspective effect.
pub fn skew_about(sx_deg: f32, sy_deg: f32, cx: f32, cy: f32) -> Aff {
    let (tx, ty) = (sx_deg.to_radians().tan(), sy_deg.to_radians().tan());
    // T(c) * Sk * T(-c).
    let (sx, sy) = (tx.clamp(-2.0, 2.0), ty.clamp(-2.0, 2.0));
    Aff {
        a: 1.0,
        b: sy,
        c: sx,
        d: 1.0,
        tx: -sx * cy,
        ty: -sy * cx,
    }
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

pub struct FloatBuf {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Px>,
}

impl FloatBuf {
    pub fn clear(w: u32, h: u32) -> Self {
        Self { w, h, px: vec![Px::clear(); (w.max(1) * h.max(1)) as usize] }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Px {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return Px::clear();
        }
        self.px[(y as u32 * self.w + x as u32) as usize]
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, p: Px) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        self.px[(y as u32 * self.w + x as u32) as usize] = p;
    }

    /// Bilinear sample in pixel space (straight alpha).
    pub fn sample(&self, x: f32, y: f32) -> Px {
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fx = (x - x0 as f32).clamp(0.0, 1.0);
        let fy = (y - y0 as f32).clamp(0.0, 1.0);
        let a = self.get(x0, y0);
        let b = self.get(x0 + 1, y0);
        let c = self.get(x0, y0 + 1);
        let d = self.get(x0 + 1, y0 + 1);
        let mix = |p: f32, q: f32, r: f32, s: f32| {
            p * (1.0 - fx) * (1.0 - fy) + q * fx * (1.0 - fy) + r * (1.0 - fx) * fy + s * fx * fy
        };
        Px {
            r: mix(a.r, b.r, c.r, d.r),
            g: mix(a.g, b.g, c.g, d.g),
            b: mix(a.b, b.b, c.b, d.b),
            a: mix(a.a, b.a, c.a, d.a),
        }
    }

    pub fn average(&self) -> Color {
        if self.px.is_empty() {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0, 0.0, 0.0);
        for p in &self.px {
            r += p.r as f64;
            g += p.g as f64;
            b += p.b as f64;
            a += p.a as f64;
        }
        let n = self.px.len() as f64;
        let avg_a = (a / n) as f32;
        if avg_a <= 1e-6 {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        // Un-premultiply the mean for a straight average color.
        Color::rgba(
            ((r / n) as f32 / avg_a).clamp(0.0, 1.0),
            ((g / n) as f32 / avg_a).clamp(0.0, 1.0),
            ((b / n) as f32 / avg_a).clamp(0.0, 1.0),
            avg_a.clamp(0.0, 1.0),
        )
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let (r, g, b) = if p.a <= 1e-6 {
                (0.0, 0.0, 0.0)
            } else {
                (p.r / p.a, p.g / p.a, p.b / p.a)
            };
            out.push((r.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((g.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((b.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((p.a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }
}

pub fn png_encode(w: u32, h: u32, rgba8: &[u8]) -> Vec<u8> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(
        &mut out,
        CompressionType::Fast,
        FilterType::Sub,
    );
    // Fall back to raw bytes on encode error (never expected).
    if enc
        .write_image(rgba8, w.max(1), h.max(1), image::ExtendedColorType::Rgba8)
        .is_err()
    {
        return rgba8.to_vec();
    }
    out
}

// ---------------------------------------------------------------------------
// Shape SDF fills (local px, straight alpha)
// ---------------------------------------------------------------------------

fn fill_rect(buf: &mut FloatBuf, w: f32, h: f32, cr: f32, col: Px) {
    let cr = cr.clamp(0.0, w.min(h) / 2.0);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if fx > w || fy > h {
                continue;
            }
            let inside = if cr <= 0.0 {
                true
            } else {
                let cx = fx.clamp(cr, w - cr);
                let cy = fy.clamp(cr, h - cr);
                let dx = fx - cx;
                let dy = fy - cy;
                dx * dx + dy * dy <= cr * cr + 0.5
            };
            if inside {
                // Cheap 1px AA on the outer edge.
                let edge = (w - fx).min(fx).min(h - fy).min(fy);
                let mut p = col;
                if edge < 1.0 && edge > 0.0 {
                    p.scale(edge.clamp(0.0, 1.0));
                }
                let dst = buf.get(x as i32, y as i32);
                let mut out = dst;
                out.over(p);
                buf.put(x as i32, y as i32, out);
            }
        }
    }
}

fn fill_ellipse(buf: &mut FloatBuf, rx: f32, ry: f32, col: Px) {
    let (cx, cy) = (buf.w as f32 / 2.0, buf.h as f32 / 2.0);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let fx = (x as f32 + 0.5 - cx) / rx.max(0.5);
            let fy = (y as f32 + 0.5 - cy) / ry.max(0.5);
            let d = fx * fx + fy * fy;
            if d <= 1.0 {
                let mut p = col;
                // Smooth rim AA.
                let rim = ((1.0 - d).max(0.0) * rx.min(ry) * 0.5).min(1.0);
                if rim < 1.0 {
                    p.scale(rim.clamp(0.15, 1.0));
                }
                let dst = buf.get(x as i32, y as i32);
                let mut out = dst;
                out.over(p);
                buf.put(x as i32, y as i32, out);
            }
        }
    }
}

/// Stroke a polyline path (`M x y L x y ...`) with round-ish 2px nib.
fn stroke_path(buf: &mut FloatBuf, path_data: &str, nib: f32, col: Px) {
    let pts = parse_path_points(path_data);
    if pts.len() < 2 {
        // Single point: draw a dot.
        if let Some(&(x, y)) = pts.first() {
            dot(buf, x, y, nib, col);
        }
        return;
    }
    for w in pts.windows(2) {
        stroke_segment(buf, w[0], w[1], nib, col);
    }
}

fn parse_path_points(path_data: &str) -> Vec<(f32, f32)> {
    let tokens: Vec<&str> = path_data.split_whitespace().collect();
    let mut pts = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "M" | "L" => {
                if i + 2 < tokens.len() {
                    if let (Ok(x), Ok(y)) =
                        (tokens[i + 1].parse::<f32>(), tokens[i + 2].parse::<f32>())
                    {
                        pts.push((x, y));
                    }
                    i += 3;
                } else {
                    break;
                }
            }
            "Z" | "z" => {
                if let Some(&first) = pts.first() {
                    pts.push(first);
                }
                i += 1;
            }
            _ => {
                // Bare coordinate pair.
                if i + 1 < tokens.len() {
                    if let (Ok(x), Ok(y)) = (tokens[i].parse::<f32>(), tokens[i + 1].parse::<f32>()) {
                        pts.push((x, y));
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            }
        }
    }
    pts
}

fn dot(buf: &mut FloatBuf, x: f32, y: f32, r: f32, col: Px) {
    let r2 = r * r;
    for oy in (-r.ceil() as i32)..=(r.ceil() as i32) {
        for ox in (-r.ceil() as i32)..=(r.ceil() as i32) {
            let dx = ox as f32 + 0.5;
            let dy = oy as f32 + 0.5;
            if dx * dx + dy * dy <= r2 {
                let dst = buf.get(x as i32 + ox, y as i32 + oy);
                let mut out = dst;
                out.over(col);
                buf.put(x as i32 + ox, y as i32 + oy, out);
            }
        }
    }
}

fn stroke_segment(buf: &mut FloatBuf, a: (f32, f32), b: (f32, f32), nib: f32, col: Px) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = (dx * dx + dy * dy).sqrt();
    let steps = (len.max(1.0)).ceil() as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        dot(buf, a.0 + dx * t, a.1 + dy * t, nib, col);
    }
}

// ---------------------------------------------------------------------------
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
                // to LayoutGlyph::physical — do NOT add line_y again.
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

/// Gaussian blur a straight-alpha buffer in place (delegates to the
/// tested renderer CPU kernel on packed bytes).
pub fn blur_buffer(buf: &mut FloatBuf, radius_px: f32) {
    if radius_px < 0.5 || buf.w == 0 || buf.h == 0 {
        return;
    }
    let mut bytes = buf.to_rgba8();
    renderer::gaussian_blur_rgba(&mut bytes, buf.w, buf.h, radius_px);
    for (i, p) in buf.px.iter_mut().enumerate() {
        let a = bytes[i * 4 + 3] as f32 / 255.0;
        *p = Px {
            r: bytes[i * 4] as f32 / 255.0 * a,
            g: bytes[i * 4 + 1] as f32 / 255.0 * a,
            b: bytes[i * 4 + 2] as f32 / 255.0 * a,
            a,
        };
    }
}

// ---------------------------------------------------------------------------
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
    extra: u64,
) -> u64 {
    let mut h = DefaultHasher::new();
    layer.id.hash(&mut h);
    frame.hash(&mut h);
    out_w.hash(&mut h);
    out_h.hash(&mut h);
    playing.hash(&mut h);
    extra.hash(&mut h);
    format!("{:?}", layer).hash(&mut h);
    h.finish()
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
            apply_adjustment(&mut dst, &layer.effects);
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

/// Base content dims (mirror the viewer estimate for pivot consistency).
fn layer_base_dims(
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

fn raster_layer_content(
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

fn apply_layer_fx(
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

fn apply_bloom(buf: &mut FloatBuf, intensity: f32, radius_px: f32) {
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
fn apply_adjustment(buf: &mut FloatBuf, effects: &[EvaluatedEffect]) {
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
                    let a = p.a;
                    *p = Px::from_color(c);
                    p.a = a;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_invert_round_trip() {        let m = Aff { a: 2.0, b: 0.5, c: -1.0, d: 3.0, tx: 10.0, ty: -4.0 };
        let inv = aff_invert(m).unwrap();
        let (x, y) = (7.0, -3.0);
        let (wx, wy) = aff_apply(m, x, y);
        let (rx, ry) = aff_apply(inv, wx, wy);
        assert!((rx - x).abs() < 1e-4 && (ry - y).abs() < 1e-4);
        // Singular matrix has no inverse.
        assert!(aff_invert(Aff { a: 0.0, b: 0.0, c: 0.0, d: 0.0, tx: 0.0, ty: 0.0 }).is_none());
    }

    #[test]
    fn affine_mul_and_skew() {
        let id = Aff { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };
        let m = Aff { a: 2.0, b: 0.0, c: 1.0, d: 3.0, tx: 5.0, ty: -2.0 };
        let r = aff_mul(id, m);
        assert!((r.a - 2.0).abs() < 1e-5 && (r.tx - 5.0).abs() < 1e-5);
        let r2 = aff_mul(m, id);
        assert!((r2.d - 3.0).abs() < 1e-5 && (r2.ty + 2.0).abs() < 1e-5);
        // Zero skew is identity; 45-degree skew shears x by y.
        let s0 = skew_about(0.0, 0.0, 10.0, 10.0);
        let (zx, zy) = aff_apply(s0, 4.0, 7.0);
        assert!((zx - 4.0).abs() < 1e-4 && (zy - 7.0).abs() < 1e-4);
        let s45 = skew_about(45.0, 0.0, 0.0, 0.0);
        let (kx, ky) = aff_apply(s45, 3.0, 2.0);
        assert!((kx - 5.0).abs() < 1e-4 && (ky - 2.0).abs() < 1e-4);
        // Pivot fixed point: center maps to itself.
        let (px, py) = aff_apply(skew_about(20.0, -10.0, 50.0, 40.0), 50.0, 40.0);
        assert!((px - 50.0).abs() < 1e-3 && (py - 40.0).abs() < 1e-3);
    }

    #[test]
    fn blend_modes_sanity() {
        let white = [1.0, 1.0, 1.0];
        let black = [0.0, 0.0, 0.0];
        let half = [0.5, 0.5, 0.5];
        assert_eq!(blend_color(BlendMode::Multiply, white, half), [0.5, 0.5, 0.5]);
        assert_eq!(blend_color(BlendMode::Screen, black, half), [0.5, 0.5, 0.5]);
        assert_eq!(blend_color(BlendMode::Add, half, half), [1.0, 1.0, 1.0]);
        let d = blend_color(BlendMode::Overlay, [0.2, 0.2, 0.2], [0.8, 0.8, 0.8]);
        assert!(d[0] > 0.2 && d[0] < 1.0);
    }

    #[test]
    fn rect_and_ellipse_fill() {
        let mut buf = FloatBuf::clear(20, 20);
        fill_rect(&mut buf, 20.0, 20.0, 0.0, Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
        assert!(buf.get(10, 10).r > 0.9);
        let red = buf.px.iter().filter(|p| p.r > 0.9 && p.a > 0.9).count();
        assert!(red > 300, "red {red}");
        let mut buf = FloatBuf::clear(21, 21);
        fill_ellipse(&mut buf, 10.0, 10.0, Px { r: 0.0, g: 1.0, b: 0.0, a: 1.0 });
        assert!(buf.get(10, 10).g > 0.9);
        assert!(buf.get(0, 0).a < 0.5);
    }

    #[test]
    fn text_rasterizes_with_sane_ink() {
        let spec = TextSpec {
            text: "Ag",
            family: "Arial",
            size: 64.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 0.0,
            align: TextAlign::Left,
            all_caps: false,
            stroke_w: 0.0,
            stroke_col: Px::clear(),
            baseline_shift: 0.0,
            box_w: 0.0,
            bevel: None,
        };
        let (buf, (x0, y0, x1, y1)) = raster_text(&spec);
        assert!(x1 > x0 && y1 > y0, "text must leave ink");
        let (iw, ih) = (x1 - x0, y1 - y0);
        // Plausible glyph extents for 64px caps text.
        assert!(iw > 30.0 && iw < 300.0, "ink width {iw}");
        assert!(ih > 25.0 && ih < 110.0, "ink height {ih}");
        // Ink sits in the upper portion (baseline layout, not sunk).
        assert!(y0 < buf.h as f32 * 0.6, "ink top {y0} of {}", buf.h);
        assert!(x0 < buf.w as f32 * 0.4, "ink left {x0}");
        let _ = (x0, y0, x1, y1);
    }

    #[test]
    fn checker_and_gradient_cover() {
        let mut buf = FloatBuf::clear(32, 32);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let fx = EvaluatedEffectType::Checkerboard {
            size: 8.0,
            color_a: Color::BLACK,
            color_b: Color::WHITE,
        };
        let ctx = RasterFx { time_s: 0.0, frame: 0, res_w: 32.0, res_h: 32.0, duration_s: 0.0, playing: false };
        apply_effect_pixels(&mut buf, 32.0, 32.0, &fx, &ctx);
        let dark = buf.px.iter().filter(|p| p.r < 0.1).count();
        let light = buf.px.iter().filter(|p| p.r > 0.9).count();
        assert!(dark > 100 && light > 100, "{dark} {light}");
        let mut buf = FloatBuf::clear(32, 1);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let fx = EvaluatedEffectType::GradientRamp {
            color_a: Color::BLACK,
            color_b: Color::WHITE,
            angle: 0.0,
        };
        apply_effect_pixels(&mut buf, 32.0, 1.0, &fx, &ctx);
        assert!(buf.px[0].r < 0.1);
        assert!(buf.px[31].r > 0.9);
    }
}
