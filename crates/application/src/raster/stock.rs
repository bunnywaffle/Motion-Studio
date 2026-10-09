//! Stock plug-in spatial kernels (neighbours / position / time).
//!
//! Self-contained per-effect implementations sharing small helpers
//! (snapshot remap, box/1D blur, highlight masks, value noise, morphology).
//! Per-pixel stock math lives in `compositor::fx`; WGSL twins in
//! `renderer::effect_filters`. All three agree on param order (descriptor
//! index) — see the parity notes on each kernel.

use super::buffer::{FloatBuf, blur_buffer};
use super::effects::RasterFx;
use super::pixel::Px;
use compositor::fx::stock_p;
use project::{Color, StockPlugin};

/// Snapshot a buffer for read-while-write kernels.
fn snap(buf: &FloatBuf) -> FloatBuf {
    FloatBuf { w: buf.w, h: buf.h, px: buf.px.clone() }
}

/// Resample `buf` through an inverse map: `f(x, y)` returns the source
/// coords (buffer px) for output pixel `(x, y)`.
fn remap(buf: &mut FloatBuf, f: impl Fn(f32, f32, f32, f32) -> (f32, f32)) {
    if buf.w == 0 || buf.h == 0 {
        return;
    }
    let src = snap(buf);
    let (w, h) = (buf.w as f32, buf.h as f32);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let (sx, sy) = f(x as f32, y as f32, w, h);
            buf.px[(y * buf.w + x) as usize] = src.sample(sx, sy);
        }
    }
}

/// Straight-alpha luminance of a premultiplied pixel.
fn pluma(p: &Px) -> f32 {
    let ia = 1.0 / p.a.max(1e-6);
    (0.299 * p.r + 0.587 * p.g + 0.114 * p.b) * ia
}

/// Fast box blur via integral image (uniform kernel, radius in px).
pub fn box_blur(buf: &mut FloatBuf, radius: f32) {
    let r = radius.clamp(0.0, 64.0).floor() as i32;
    if r < 1 || buf.w == 0 || buf.h == 0 {
        return;
    }
    let (w, h) = (buf.w as usize, buf.h as usize);
    // Summed-area tables per channel.
    let mut sat = vec![[0.0f64; 4]; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            let p = buf.px[y * w + x];
            let i = (y + 1) * (w + 1) + (x + 1);
            sat[i] = [
                sat[i - 1][0] + sat[i - w - 1][0] - sat[i - w - 2][0] + p.r as f64,
                sat[i - 1][1] + sat[i - w - 1][1] - sat[i - w - 2][1] + p.g as f64,
                sat[i - 1][2] + sat[i - w - 1][2] - sat[i - w - 2][2] + p.b as f64,
                sat[i - 1][3] + sat[i - w - 1][3] - sat[i - w - 2][3] + p.a as f64,
            ];
        }
    }
    let at = |x: i32, y: i32| -> [f64; 4] {
        let x = x.clamp(0, w as i32) as usize;
        let y = y.clamp(0, h as i32) as usize;
        sat[y * (w + 1) + x]
    };
    for y in 0..h {
        for x in 0..w {
            let (x0, y0, x1, y1) = (x as i32 - r, y as i32 - r, x as i32 + r + 1, y as i32 + r + 1);
            let a = at(x0, y0);
            let b = at(x1, y0);
            let c = at(x0, y1);
            let d = at(x1, y1);
            let n = ((x1.min(w as i32) - x0.max(0)) * (y1.min(h as i32) - y0.max(0))).max(1) as f64;
            buf.px[y * w + x] = Px {
                r: ((d[0] - b[0] - c[0] + a[0]) / n) as f32,
                g: ((d[1] - b[1] - c[1] + a[1]) / n) as f32,
                b: ((d[2] - b[2] - c[2] + a[2]) / n) as f32,
                a: ((d[3] - b[3] - c[3] + a[3]) / n) as f32,
            };
        }
    }
}

/// Directional (1D) blur along `angle_deg` with `length` px spread.
pub fn dir_blur(buf: &mut FloatBuf, angle_deg: f32, length: f32) {
    let len = length.clamp(0.0, 200.0);
    if len < 0.5 {
        return;
    }
    let rad = angle_deg.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let taps = (len as usize).clamp(2, 48);
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let mut acc = Px::clear();
            for i in 0..taps {
                let t = (i as f32 / (taps - 1) as f32 - 0.5) * len;
                let s = src.sample(x as f32 + dx * t, y as f32 + dy * t);
                acc.r += s.r;
                acc.g += s.g;
                acc.b += s.b;
                acc.a += s.a;
            }
            let n = taps as f32;
            buf.px[(y * buf.w + x) as usize] = Px { r: acc.r / n, g: acc.g / n, b: acc.b / n, a: acc.a / n };
        }
    }
}

/// White-where-bright mask buffer (for light effects).
fn highlights(src: &FloatBuf, threshold: f32) -> FloatBuf {
    let th = (threshold / 100.0).clamp(0.0, 1.0);
    let mut m = FloatBuf::clear(src.w, src.h);
    for (d, s) in m.px.iter_mut().zip(src.px.iter()) {
        let l = pluma(s);
        if l > th {
            let k = ((l - th) / (1.0 - th).max(1e-3)).clamp(0.0, 1.0);
            *d = Px { r: k * s.a, g: k * s.a, b: k * s.a, a: k * s.a };
        }
    }
    m
}

/// Screen-blend `glow` over `buf` scaled by `k`.
fn screen_over(buf: &mut FloatBuf, glow: &FloatBuf, k: f32) {
    for (d, g) in buf.px.iter_mut().zip(glow.px.iter()) {
        if d.a <= 0.0 || g.a <= 0.0 {
            continue;
        }
        let ia = 1.0 / d.a.max(1e-6);
        let ib = 1.0 / g.a.max(1e-6);
        let s = |x: f32, y: f32| 1.0 - (1.0 - x) * (1.0 - y * k);
        d.r = s(d.r * ia, g.r * ib).clamp(0.0, 1.0) * d.a;
        d.g = s(d.g * ia, g.g * ib).clamp(0.0, 1.0) * d.a;
        d.b = s(d.b * ia, g.b * ib).clamp(0.0, 1.0) * d.a;
    }
}

/// Deterministic 2D hash in [0, 1).
pub fn hash2(x: f32, y: f32, seed: f32) -> f32 {
    ((x * 12.9898 + y * 78.233 + seed * 37.719).sin() * 43_758.547).fract()
}

/// Smooth value noise in [0, 1].
pub fn vnoise(x: f32, y: f32, seed: f32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let xf = x - xi;
    let yf = y - yi;
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1.0, yi, seed);
    let c = hash2(xi, yi + 1.0, seed);
    let d = hash2(xi + 1.0, yi + 1.0, seed);
    a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + c * (1.0 - u) * v + d * u * v
}

/// Fractal Brownian motion in [0, 1].
pub fn fbm(x: f32, y: f32, octaves: usize, seed: f32) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.5;
    let mut fx = x;
    let mut fy = y;
    for i in 0..octaves.clamp(1, 8) {
        v += amp * vnoise(fx, fy, seed + i as f32 * 13.7);
        amp *= 0.5;
        fx = fx * 2.03 + 17.3;
        fy = fy * 2.03 + 9.1;
    }
    v.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Blur kernels
// ---------------------------------------------------------------------------

fn k_radial(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, zoom: bool) {
    let amount = (stock_p(plugin, p, 0) / 100.0).clamp(0.0, 1.0);
    let fall = (stock_p(plugin, p, 1) / 100.0).clamp(0.0, 1.0);
    if amount <= 0.001 {
        return;
    }
    let src = snap(buf);
    let (w, h) = (buf.w as f32, buf.h as f32);
    let (cx, cy) = (w * 0.5, h * 0.5);
    let max_r = w.min(h) * 0.5;
    let taps = 16;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt() / max_r.max(1.0);
            let edge = (r * (1.0 - fall * 0.7)).clamp(0.0, 1.0);
            let strength = amount * edge * max_r * 0.12;
            if strength < 0.25 {
                continue;
            }
            let (ux, uy) = if r < 1e-5 {
                (0.0, 0.0)
            } else {
                (dx / (r * max_r.max(1.0)), dy / (r * max_r.max(1.0)))
            };
            let mut acc = Px::clear();
            for i in 0..taps {
                let t = (i as f32 / (taps - 1) as f32 - 0.5) * strength;
                // Zoom blurs along the ray; radial blurs around it.
                let (ox, oy) = if zoom { (ux * t, uy * t) } else { (-uy * t, ux * t) };
                let s = src.sample(x as f32 + ox, y as f32 + oy);
                acc.r += s.r;
                acc.g += s.g;
                acc.b += s.b;
                acc.a += s.a;
            }
            let n = taps as f32;
            buf.px[(y * buf.w + x) as usize] =
                Px { r: acc.r / n, g: acc.g / n, b: acc.b / n, a: acc.a / n };
        }
    }
}

fn k_defocus(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    // Fast defocus: two box passes approximate a disk kernel.
    let r = stock_p(plugin, p, 0).clamp(0.0, 40.0);
    if r < 0.5 {
        return;
    }
    box_blur(buf, r);
    box_blur(buf, r * 0.7);
}

fn k_bokeh(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let r = stock_p(plugin, p, 0).clamp(0.0, 40.0);
    let th = stock_p(plugin, p, 1).clamp(0.0, 100.0);
    if r < 0.5 {
        return;
    }
    // Blurred highlights screened back: bright discs bloom, darks stay crisp.
    let mut hi = highlights(&snap(buf), th);
    box_blur(&mut hi, r);
    box_blur(&mut hi, r * 0.6);
    screen_over(buf, &hi, 0.9);
    box_blur(buf, r * 0.35);
}

fn k_bilateral(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let r = stock_p(plugin, p, 0).clamp(0.0, 10.0).min(4.0) as i32;
    let range = (stock_p(plugin, p, 1) / 100.0).clamp(0.01, 1.0);
    if r < 1 {
        return;
    }
    // Two range-weighted passes (H then V via transposed walk below);
    // radius capped for preview speed.
    for pass in 0..2 {
        let src = snap(buf);
        let horiz = pass == 0;
        for y in 0..buf.h {
            for x in 0..buf.w {
                let c = src.px[(y * buf.w + x) as usize];
                let mut acc = Px { r: c.r, g: c.g, b: c.b, a: c.a };
                let mut wsum = 1.0;
                for i in -r..=r {
                    if i == 0 {
                        continue;
                    }
                    let (sx, sy) = if horiz {
                        (x as i32 + i, y as i32)
                    } else {
                        (x as i32, y as i32 + i)
                    };
                    if sx < 0 || sy < 0 || sx >= buf.w as i32 || sy >= buf.h as i32 {
                        continue;
                    }
                    let s = src.px[(sy as u32 * buf.w + sx as u32) as usize];
                    let dr = (pluma(&c) - pluma(&s)).abs();
                    let w = (-dr / range).exp();
                    acc.r += s.r * w;
                    acc.g += s.g * w;
                    acc.b += s.b * w;
                    acc.a += s.a * w;
                    wsum += w;
                }
                buf.px[(y * buf.w + x) as usize] =
                    Px { r: acc.r / wsum, g: acc.g / wsum, b: acc.b / wsum, a: acc.a / wsum };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Light kernels
// ---------------------------------------------------------------------------

/// Shared glow core: blurred highlights screened back.
fn glow_core(buf: &mut FloatBuf, intensity: f32, radius: f32, threshold: f32) {
    let k = (intensity / 100.0).clamp(0.0, 1.0);
    if k <= 0.001 {
        return;
    }
    let mut hi = highlights(&snap(buf), threshold);
    blur_buffer(&mut hi, radius.clamp(0.5, 2048.0));
    screen_over(buf, &hi, k);
}

fn k_glow(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    glow_core(buf, stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
}

/// Anamorphic-style streaks: long 1D smear of the highlights.
fn streak_layer(buf: &FloatBuf, length: f32, angle: f32, threshold: f32) -> FloatBuf {
    let mut hi = highlights(buf, threshold);
    dir_blur(&mut hi, angle, length);
    hi
}

fn k_glare(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (inten, len, ang) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    if inten <= 0.01 {
        return;
    }
    glow_core(buf, inten * 0.7, len * 0.25 + 2.0, 55.0);
    let st = streak_layer(&snap(buf), len, ang, 60.0);
    screen_over(buf, &st, (inten / 100.0).clamp(0.0, 1.0) * 0.8);
    let st2 = streak_layer(&snap(buf), len * 0.6, ang + 90.0, 65.0);
    screen_over(buf, &st2, (inten / 100.0).clamp(0.0, 1.0) * 0.5);
}

fn k_glint(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (inten, size) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1));
    if inten <= 0.01 {
        return;
    }
    // 4-point star: tight cross streaks on the hottest pixels.
    let len = 8.0 + size * 1.6;
    let st = streak_layer(&snap(buf), len, 0.0, 80.0);
    let st2 = streak_layer(&snap(buf), len, 90.0, 80.0);
    let k = (inten / 100.0).clamp(0.0, 1.0);
    screen_over(buf, &st, k);
    screen_over(buf, &st2, k);
}

fn tint_of(colors: &[Color], i: usize, fb: Color) -> Color {
    colors.get(i).copied().unwrap_or(fb)
}

fn k_light_rays(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let inten = stock_p(plugin, p, 0);
    let len = stock_p(plugin, p, 1);
    let ang = stock_p(plugin, p, 2);
    if inten <= 0.01 || len < 0.5 {
        return;
    }
    // New animatable params (descriptor defaults cover old project files).
    let threshold = stock_p(plugin, p, 3);
    let knee = stock_p(plugin, p, 4).clamp(0.0, 50.0).max(1.0);
    let density = 0.4 + stock_p(plugin, p, 5) / 100.0 * 1.4;
    let decay = (stock_p(plugin, p, 6) / 100.0).clamp(0.8, 1.0);
    let exposure = stock_p(plugin, p, 7) / 100.0;
    let taps = stock_p(plugin, p, 8).round().clamp(8.0, 64.0) as usize;
    let jitter = stock_p(plugin, p, 9).clamp(0.0, 1.0);
    let blend_screen = stock_p(plugin, p, 10).round() >= 1.0;
    let mix = (stock_p(plugin, p, 11) / 100.0).clamp(0.0, 1.0);
    let tint = tint_of(colors, 0, Color::rgba(1.0, 0.9, 0.7, 1.0));
    // Thresholded highlight mask with soft knee, so rays stream from hot
    // zones instead of smearing the whole frame (Chapman-style source).
    let src = snap(buf);
    let th = (threshold / 100.0).clamp(0.0, 1.0);
    let mut mask = FloatBuf::clear(src.w, src.h);
    for (d, s) in mask.px.iter_mut().zip(src.px.iter()) {
        let l = pluma(s);
        let m = ((l - th) / (knee / 100.0 + 1e-3) + 0.5).clamp(0.0, 1.0);
        let m = m * m * (3.0 - 2.0 * m);
        *d = Px { r: m * s.a, g: m * s.a, b: m * s.a, a: m * s.a };
    }
    let rad = ang.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let k = (inten / 100.0).clamp(0.0, 2.0) * mix * (0.4 + exposure);
    let mut rays = FloatBuf::clear(buf.w, buf.h);
    for y in 0..buf.h {
        for x in 0..buf.w {
            // IGN-ish per-pixel jitter of the march start kills banding at
            // low sample counts (warbell godrays.wgsl trick).
            let j = if jitter > 0.5 { hash2(x as f32, y as f32, 7.0) - 0.5 } else { 0.0 };
            let mut acc = 0.0;
            let mut wsum = 0.0;
            let mut wgt = 1.0;
            for i in 0..taps {
                let t = (i as f32 + 0.5 + j) / taps as f32 * len * density;
                let s = mask.sample(x as f32 - dx * t, y as f32 - dy * t);
                acc += s.r * wgt;
                wsum += wgt;
                wgt *= decay;
            }
            let v = (acc / wsum.max(1e-5) * k).min(2.0);
            rays.px[(y * buf.w + x) as usize] = Px {
                r: v * tint.r,
                g: v * tint.g,
                b: v * tint.b,
                a: v.clamp(0.0, 1.0),
            };
        }
    }
    if blend_screen {
        screen_over(buf, &rays, 1.0);
    } else {
        for (d, g) in buf.px.iter_mut().zip(rays.px.iter()) {
            if d.a <= 0.0 || g.a <= 0.0 {
                continue;
            }
            d.r = (d.r + g.r * d.a).min(1.0);
            d.g = (d.g + g.g * d.a).min(1.0);
            d.b = (d.b + g.b * d.a).min(1.0);
        }
    }
}

fn k_god_rays(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let inten = stock_p(plugin, p, 0);
    if inten <= 0.01 {
        return;
    }
    let dens = stock_p(plugin, p, 1);
    let decay_p = stock_p(plugin, p, 2);
    let ccx = stock_p(plugin, p, 3) / 100.0;
    let ccy = stock_p(plugin, p, 4) / 100.0;
    let threshold = stock_p(plugin, p, 5);
    let knee = stock_p(plugin, p, 6).clamp(0.0, 50.0).max(1.0);
    let weight = stock_p(plugin, p, 7) / 100.0;
    let exposure = stock_p(plugin, p, 8) / 100.0;
    let taps = stock_p(plugin, p, 9).round().clamp(8.0, 64.0) as usize;
    let jitter = stock_p(plugin, p, 10).clamp(0.0, 1.0);
    let beams = (stock_p(plugin, p, 11) / 100.0).clamp(0.0, 1.0);
    let tint = tint_of(colors, 0, Color::rgba(1.0, 0.9, 0.7, 1.0));
    let src = snap(buf);
    let th = (threshold / 100.0).clamp(0.0, 1.0);
    let mut mask = FloatBuf::clear(src.w, src.h);
    for (d, s) in mask.px.iter_mut().zip(src.px.iter()) {
        let l = pluma(s);
        let m = ((l - th) / (knee / 100.0 + 1e-3) + 0.5).clamp(0.0, 1.0);
        let m = m * m * (3.0 - 2.0 * m);
        *d = Px { r: m * s.a, g: m * s.a, b: m * s.a, a: m * s.a };
    }
    let (w, h) = (buf.w as f32, buf.h as f32);
    let (cx, cy) = (w * ccx, h * ccy);
    let density = 0.5 + dens / 100.0 * 1.5;
    let dk = decay_p.clamp(0.8, 1.0);
    let k = (inten / 100.0).clamp(0.0, 2.0) * (0.3 + weight * 1.4) * (0.4 + exposure);
    let mut rays = FloatBuf::clear(buf.w, buf.h);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let j = if jitter > 0.5 { hash2(x as f32, y as f32, 3.0) - 0.5 } else { 0.0 };
            let mut acc = 0.0;
            let mut wsum = 0.0;
            let mut wgt = 1.0;
            for i in 0..taps {
                let t = (i as f32 + 0.5 + j) / taps as f32 * density;
                let s = mask.sample(cx + dx * (1.0 - t * 0.5), cy + dy * (1.0 - t * 0.5));
                acc += s.r * wgt;
                wsum += wgt;
                wgt *= dk;
            }
            // Angular shaft structure: non-harmonic beam/gap modulation
            // anchored to the light direction (warbell beam trick).
            let mut v = acc / wsum.max(1e-5) * k;
            if beams > 0.01 {
                let ang = dy.atan2(dx);
                let beam = 0.72 + 0.28 * (0.6 * (ang * 9.0).sin() + 0.4 * (ang * 17.0 + 1.7).sin());
                v *= 1.0 - beams + beams * beam;
            }
            rays.px[(y * buf.w + x) as usize] = Px {
                r: (v * tint.r).min(2.0),
                g: (v * tint.g).min(2.0),
                b: (v * tint.b).min(2.0),
                a: v.clamp(0.0, 1.0),
            };
        }
    }
    for (d, g) in buf.px.iter_mut().zip(rays.px.iter()) {
        if d.a <= 0.0 || g.a <= 0.0 {
            continue;
        }
        d.r = (d.r + g.r * d.a).min(1.0);
        d.g = (d.g + g.g * d.a).min(1.0);
        d.b = (d.b + g.b * d.a).min(1.0);
    }
}

fn k_lens_flare(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let inten = stock_p(plugin, p, 0);
    if inten <= 0.01 {
        return;
    }
    let pos = stock_p(plugin, p, 2);
    let k = (inten / 100.0).clamp(0.0, 2.0);
    let (w, h) = (buf.w as f32, buf.h as f32);
    let ccx = stock_p(plugin, p, 3) / 100.0;
    let ccy = stock_p(plugin, p, 4) / 100.0;
    let threshold = stock_p(plugin, p, 5);
    let dispersal = 0.4 + stock_p(plugin, p, 6) / 100.0 * 1.4;
    let halo_w = stock_p(plugin, p, 7) / 100.0;
    let halo_k = (stock_p(plugin, p, 8) / 100.0).clamp(0.0, 2.0);
    let chroma = stock_p(plugin, p, 9).clamp(0.0, 8.0);
    let streak_len = stock_p(plugin, p, 10);
    let streak_k = (stock_p(plugin, p, 11) / 100.0).clamp(0.0, 2.0);
    let tint = tint_of(colors, 0, Color::rgba(1.0, 0.9, 0.75, 1.0));
    // Hot-spot slides with Position along the top third (legacy behavior);
    // explicit center overrides the auto frame center.
    let t = (pos / 100.0).clamp(0.0, 1.0);
    let (hx, hy) = (w * (0.2 + 0.6 * t), h * 0.35);
    let (cx, cy) = (w * ccx, h * ccy);
    let ghosts = stock_p(plugin, p, 2).round().clamp(0.0, 10.0) as usize;
    // Gate the whole flare on scene heat so dark frames stay clean
    // (threshold over mean luminance, Chapman bright-pass idea).
    let mut mean = 0.0;
    let mut n = 0;
    for s in snap(buf).px.iter().step_by(16) {
        mean += pluma(s) * s.a;
        n += 1;
    }
    let heat = (((mean / n.max(1) as f32) - threshold / 100.0) * 4.0 + 0.5).clamp(0.0, 1.0);
    let gate = heat * heat * (3.0 - 2.0 * heat);
    if gate <= 0.01 {
        return;
    }
    let min_d = w.min(h).max(1.0);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            if buf.px[idx].a <= 0.0 {
                continue;
            }
            let mut ar = 0.0;
            let mut ag = 0.0;
            let mut ab = 0.0;
            let dh = ((x as f32 - hx).powi(2) + (y as f32 - hy).powi(2)).sqrt() / min_d;
            let hot = (-dh * dh * 60.0).exp() * 1.2;
            ar += hot;
            ag += hot;
            ab += hot;
            // Ghost discs mirrored across the center, spread by dispersal;
            // chroma offsets the per-channel radii (cheap CA fringing).
            for i in 1..=ghosts {
                let f = i as f32 / ghosts.max(1) as f32 * dispersal;
                let gx = cx + (cx - hx) * f * 1.4;
                let gy = cy + (cy - hy) * f * 1.4;
                let r = 0.02 + 0.05 * (i as f32 / ghosts.max(1) as f32);
                let ring = |dd: f32| (-((dd - r) * (dd - r)) / (0.004 + 0.01 * r)).exp() * 0.35;
                let dg = ((x as f32 - gx).powi(2) + (y as f32 - gy).powi(2)).sqrt() / min_d;
                let px = chroma * 0.004;
                ar += ring((dg - px).max(0.0));
                ag += ring(dg);
                ab += ring(dg + px);
            }
            let dc = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / min_d;
            let halo = (-((dc - 0.30 * (0.5 + halo_w)) / (0.12 + halo_w * 0.2 + 1e-3)).powi(2)).exp() * halo_k;
            ar += halo;
            ag += halo * 0.9;
            ab += halo * 0.8;
            let p = &mut buf.px[idx];
            let g = (k * gate * p.a).min(1.5);
            p.r = (p.r + ar * g * tint.r).min(1.0);
            p.g = (p.g + ag * g * tint.g).min(1.0);
            p.b = (p.b + ab * g * tint.b).min(1.0);
        }
    }
    // Anamorphic streaks off the highlights reuse the 1D smear helper.
    if streak_k > 0.01 && streak_len > 0.5 {
        let st = streak_layer(&snap(buf), streak_len.max(8.0), 0.0, 60.0);
        screen_over(buf, &st, streak_k * gate * 0.7);
    }
}

fn k_light_leak(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    use compositor::fx::process_color_stock;
    let (inten, hue, pos) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    let k = (inten / 100.0).clamp(0.0, 1.0);
    if k <= 0.001 {
        return;
    }
    let (w, h) = (buf.w as f32, buf.h as f32);
    let t = (pos / 100.0).clamp(0.0, 1.0);
    // Leak origin slides along the top edge; falloff is diagonal.
    let (ox, oy) = (w * t, -h * 0.1);
    let diag = (w * w + h * h).sqrt();
    // Warm the leak color via the hue kernel on orange.
    let warm = process_color_stock(
        StockPlugin::TemperatureTint,
        &[hue, 0.0],
        Color::rgba(1.0, 0.45, 0.15, 1.0),
    );
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = buf.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let dist = ((x as f32 - ox).powi(2) + (y as f32 - oy).powi(2)).sqrt() / diag;
            let m = ((1.0 - dist * 1.6).clamp(0.0, 1.0)).powf(1.5) * k;
            let ia = 1.0 / d.a.max(1e-6);
            let s = |a: f32, b: f32| 1.0 - (1.0 - a) * (1.0 - b * m);
            buf.px[idx] = Px {
                r: s(d.r * ia, warm.r).clamp(0.0, 1.0) * d.a,
                g: s(d.g * ia, warm.g).clamp(0.0, 1.0) * d.a,
                b: s(d.b * ia, warm.b).clamp(0.0, 1.0) * d.a,
                a: d.a,
            };
        }
    }
}

fn k_streaks(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (inten, len, ang) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    if inten <= 0.01 {
        return;
    }
    let st = streak_layer(&snap(buf), len.max(8.0), ang, 55.0);
    screen_over(buf, &st, (inten / 100.0).clamp(0.0, 1.0));
}

fn k_halo(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (inten, radius, warmth) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    let k = (inten / 100.0).clamp(0.0, 1.0);
    if k <= 0.001 {
        return;
    }
    let (w, h) = (buf.w as f32, buf.h as f32);
    let r0 = (radius / 100.0).clamp(0.02, 1.0) * w.min(h) * 0.5;
    let wt = warmth / 100.0;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = buf.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let dc = ((x as f32 - w * 0.5).powi(2) + (y as f32 - h * 0.5).powi(2)).sqrt();
            let ring = (-((dc - r0) * (dc - r0)) / (2.0 * (r0 * 0.45 + 1.0).powi(2))).exp();
            let m = (ring * k * d.a).min(1.0);
            let p = &mut buf.px[idx];
            p.r = (p.r + m * (0.7 + 0.3 * wt)).min(1.0);
            p.g = (p.g + m * 0.6).min(1.0);
            p.b = (p.b + m * (0.55 - 0.3 * wt)).min(1.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Long Shadow + Saber (flat extrusion / core+bloom models)
// ---------------------------------------------------------------------------

/// Flat-extrusion long shadow: for each pixel take the max mask alpha
/// along the shadow ray, fade with distance, tint near→far. Modes:
/// 0 behind, 1 cutout (source punched out), 2 shadow only.
fn k_long_shadow(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let angle = stock_p(plugin, p, 0).to_radians();
    let dist = stock_p(plugin, p, 1).clamp(0.0, 1024.0);
    let strength = (stock_p(plugin, p, 8) / 100.0).clamp(0.0, 1.0);
    if dist < 0.5 || strength <= 0.001 {
        return;
    }
    let steps = stock_p(plugin, p, 2).round().clamp(1.0, 64.0) as usize;
    let fade = (stock_p(plugin, p, 3) / 100.0).clamp(0.0, 1.0);
    let opacity = (stock_p(plugin, p, 4) / 100.0).clamp(0.0, 1.0);
    let soft = stock_p(plugin, p, 5).clamp(0.0, 60.0);
    let expand = stock_p(plugin, p, 6).clamp(-50.0, 50.0) / 100.0;
    let mode = stock_p(plugin, p, 7).round().clamp(0.0, 2.0) as i32;
    let stride = stock_p(plugin, p, 9).clamp(1.0, 8.0);
    let near = tint_of(colors, 0, Color::rgba(0.0, 0.0, 0.0, 0.8));
    let far = tint_of(colors, 1, Color::rgba(0.0, 0.0, 0.0, 0.0));
    let (dx, dy) = (angle.cos(), angle.sin());
    let src = snap(buf);
    // Opaque mask (straight alpha + expand choke).
    let mut mask = FloatBuf::clear(src.w, src.h);
    for (d, s) in mask.px.iter_mut().zip(src.px.iter()) {
        *d = Px {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: (s.a + expand).clamp(0.0, 1.0),
        };
    }
    let exp = 1.0 + fade * 3.0;
    let mut shadow = FloatBuf::clear(buf.w, buf.h);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let mut best = 0.0;
            let mut best_t = 0.0;
            let mut t = stride;
            for _ in 0..steps {
                if t > dist {
                    break;
                }
                let m = mask.sample(x as f32 - dx * t, y as f32 - dy * t).a;
                // Pixel-art stride quantizes the ray (ChocDino step size).
                let tq = if stride > 1.0 { (t / stride).floor() * stride } else { t };
                let fall = 1.0 - (tq / dist.max(1.0)).clamp(0.0, 1.0);
                let v = m * fall.powf(exp);
                if v > best {
                    best = v;
                    best_t = (tq / dist.max(1.0)).clamp(0.0, 1.0);
                }
                t += stride;
            }
            if best > 0.003 {
                // Don't paint over the source itself in behind/cutout modes.
                let sa = src.px[(y * buf.w + x) as usize].a;
                if mode != 2 && sa > 0.5 {
                    continue;
                }
                // ponytail: single lerp near→far, no multi-stop ramp
                let a = (best * opacity * strength).min(1.0);
                shadow.px[(y * buf.w + x) as usize] = Px {
                    r: (near.r + (far.r - near.r) * best_t) * a,
                    g: (near.g + (far.g - near.g) * best_t) * a,
                    b: (near.b + (far.b - near.b) * best_t) * a,
                    a,
                };
            }
        }
    }
    if soft > 0.5 {
        blur_buffer(&mut shadow, soft);
    }
    if mode == 2 {
        *buf = shadow;
        return;
    }
    // Under-composite: shadow beneath source.
    for (d, s) in buf.px.iter_mut().zip(shadow.px.iter()) {
        if s.a <= 0.0 {
            continue;
        }
        if mode == 1 && d.a > 0.01 {
            continue;
        }
        let ia = 1.0 - s.a;
        d.r += s.r * ia.max(0.0);
        d.g += s.g * ia.max(0.0);
        d.b += s.b * ia.max(0.0);
        d.a = (d.a + s.a * (1.0 - d.a)).min(1.0);
    }
}

/// Saber: white-hot core from the alpha/luma mask + two-tone bloom glow
/// with turbulence distortion and timeline-driven flicker. Evolution is
/// manual AND auto-advanced by the frame clock (both, per request).
fn k_saber(
    buf: &mut FloatBuf,
    p: &[f32],
    plugin: StockPlugin,
    colors: &[Color],
    ctx: &RasterFx,
) {
    let core_w = stock_p(plugin, p, 0).clamp(0.0, 50.0);
    let glow_w = stock_p(plugin, p, 1).clamp(0.0, 200.0);
    let soft = (stock_p(plugin, p, 2) / 100.0).clamp(0.0, 1.0);
    let threshold = (stock_p(plugin, p, 3) / 100.0).clamp(0.0, 1.0);
    let d_amt = (stock_p(plugin, p, 4) / 100.0).clamp(0.0, 1.0);
    let d_scale = stock_p(plugin, p, 5).clamp(0.1, 10.0);
    let d_speed = stock_p(plugin, p, 6) / 100.0;
    let f_amt = (stock_p(plugin, p, 7) / 100.0).clamp(0.0, 1.0);
    let f_speed = stock_p(plugin, p, 8) / 100.0;
    let evolution = stock_p(plugin, p, 9);
    let intensity = (stock_p(plugin, p, 10) / 100.0).clamp(0.0, 2.0);
    let blend_screen = stock_p(plugin, p, 11).round() >= 1.0;
    if intensity <= 0.001 || (core_w < 0.05 && glow_w < 0.5) {
        return;
    }
    let core_c = tint_of(colors, 0, Color::WHITE);
    let inner_c = tint_of(colors, 1, Color::rgb(0.4, 0.7, 1.0));
    let outer_c = tint_of(colors, 2, Color::rgb(0.1, 0.2, 1.0));
    // Evolution: manual param + auto clock advance.
    let evo = evolution + ctx.time_s * (0.5 + d_speed * 4.0) * 30.0;
    let src = snap(buf);
    let s = d_scale * 0.03;
    // Distorted core mask.
    let mut core = FloatBuf::clear(buf.w, buf.h);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let nx = if d_amt > 0.001 {
                (vnoise(x as f32 * s + evo * 0.05, y as f32 * s, evo * 0.01) - 0.5) * d_amt * glow_w.max(8.0)
            } else {
                0.0
            };
            let ny = if d_amt > 0.001 {
                (vnoise(x as f32 * s, y as f32 * s + evo * 0.05, evo * 0.01 + 5.0) - 0.5) * d_amt * glow_w.max(8.0)
            } else {
                0.0
            };
            let sm = src.sample(x as f32 + nx, y as f32 + ny);
            let l = pluma(&sm).max(sm.a);
            let m = ((l - threshold) / 0.15 + 0.5).clamp(0.0, 1.0);
            let m = m * m * (3.0 - 2.0 * m);
            core.px[(y * buf.w + x) as usize] = Px { r: m, g: m, b: m, a: m * sm.a.max(l) };
        }
    }
    // Glow = blurred mask minus core (bloom-stack idea, glampert).
    let mut glow = core.clone();
    if glow_w > 0.5 {
        blur_buffer(&mut glow, (glow_w * (0.4 + soft * 0.9)).clamp(0.5, 64.0));
    }
    // Flicker: timeline-clocked brightness wobble, animatable amount/speed.
    let fl = if f_amt > 0.001 {
        let wob = (ctx.time_s * (1.0 + f_speed * 8.0) * 6.0).sin() * 0.5
            + (vnoise(ctx.time_s * (1.0 + f_speed * 3.0), 0.0, 1.0) - 0.5);
        1.0 + wob * f_amt * 0.6
    } else {
        1.0
    };
    let k = (intensity * fl).max(0.0);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let c = core.px[idx].a.clamp(0.0, 1.0);
            let g = glow.px[idx].a.clamp(0.0, 1.0);
            let halo = (g - c * 0.7).max(0.0);
            // ponytail: two-tone glow lerp by falloff, no spline ramp
            let t = (halo / g.max(1e-3)).clamp(0.0, 1.0);
            let gr = inner_c.r + (outer_c.r - inner_c.r) * t;
            let gg = inner_c.g + (outer_c.g - inner_c.g) * t;
            let gb = inner_c.b + (outer_c.b - inner_c.b) * t;
            let d = &mut buf.px[idx];
            if d.a <= 0.0 && c <= 0.003 && halo <= 0.003 {
                continue;
            }
            let add_r = (c * core_c.r + halo * gr) * k;
            let add_g = (c * core_c.g + halo * gg) * k;
            let add_b = (c * core_c.b + halo * gb) * k;
            if blend_screen {
                let ia = 1.0 / d.a.max(1e-6);
                let s = |a: f32, b: f32| 1.0 - (1.0 - a) * (1.0 - (b * d.a).min(1.0));
                d.r = s(d.r * ia, add_r).clamp(0.0, 1.0) * d.a;
                d.g = s(d.g * ia, add_g).clamp(0.0, 1.0) * d.a;
                d.b = s(d.b * ia, add_b).clamp(0.0, 1.0) * d.a;
            } else {
                d.r = (d.r + add_r * d.a.max(0.15)).min(1.0);
                d.g = (d.g + add_g * d.a.max(0.15)).min(1.0);
                d.b = (d.b + add_b * d.a.max(0.15)).min(1.0);
            }
            d.a = (d.a + halo * 0.5 * d.a).min(1.0);
        }
    }
    let _ = core_w;
}

// ---------------------------------------------------------------------------
// Distortion kernels (inverse-map resamples in buffer px)
// ---------------------------------------------------------------------------

fn k_turbulent(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amt, scale, seed) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    let a = amt.clamp(0.0, 200.0);
    if a < 0.05 {
        return;
    }
    let s = scale.clamp(0.1, 10.0) * 0.02;
    remap(buf, |x, y, w, h| {
        let nx = vnoise(x * s + seed * 7.0, y * s, seed);
        let ny = vnoise(x * s, y * s + seed * 3.0, seed + 5.0);
        (x + (nx - 0.5) * a * w.min(h) * 0.02, y + (ny - 0.5) * a * w.min(h) * 0.02)
    });
}

fn k_wave(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amp, wave, dir) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    if amp < 0.05 || wave < 2.0 {
        return;
    }
    let rad = dir.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let k = std::f32::consts::TAU / wave.max(2.0);
    remap(buf, |x, y, _, _| {
        let ph = (x * dx + y * dy) * k;
        (x - dy * ph.sin() * amp, y + dx * ph.sin() * amp)
    });
}

fn k_ripple(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amp, wave) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1));
    if amp < 0.05 || wave < 2.0 {
        return;
    }
    let k = std::f32::consts::TAU / wave.max(2.0);
    remap(buf, |x, y, w, h| {
        let dx = x - w * 0.5;
        let dy = y - h * 0.5;
        let r = (dx * dx + dy * dy).sqrt().max(1e-3);
        let off = (r * k).sin() * amp * (-r / (w.min(h) * 0.75)).exp();
        (x - dx / r * off, y - dy / r * off)
    });
}

/// Centered polar warp helper: `f(r01, theta)` returns `(r01_out, theta_out)`
/// mapping OUTPUT polar to SOURCE polar.
fn polar_remap(buf: &mut FloatBuf, radius_pct: f32, f: impl Fn(f32, f32) -> (f32, f32)) {
    let r_max = (radius_pct / 100.0).clamp(0.02, 1.0);
    remap(buf, |x, y, w, h| {
        let (cx, cy) = (w * 0.5, h * 0.5);
        let m = w.min(h) * 0.5;
        let dx = x - cx;
        let dy = y - cy;
        let r = (dx * dx + dy * dy).sqrt() / m;
        if r > r_max || r < 1e-5 {
            return (x, y);
        }
        let th = dy.atan2(dx);
        let t = r / r_max;
        let (rt, tht) = f(t, th);
        let rn = (rt * r_max * m).min(m * 1.5);
        (cx + rn * tht.cos(), cy + rn * tht.sin())
    });
}

fn k_twirl(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (ang, rad) = (stock_p(plugin, p, 0).to_radians(), stock_p(plugin, p, 1));
    if ang.abs() < 0.001 {
        return;
    }
    polar_remap(buf, rad, |t, th| (t, th + ang * (1.0 - t)));
}

fn k_bulge(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amt, rad) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1));
    if amt.abs() < 0.001 {
        return;
    }
    polar_remap(buf, rad, |t, th| {
        // Outward (positive) or inward (negative) dome.
        let r = t + amt * (1.0 - t * t) * 0.35 * (1.0 - t);
        (r.max(0.0), th)
    });
}

fn k_spherize(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amt, rad) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1));
    if amt.abs() < 0.001 {
        return;
    }
    polar_remap(buf, rad, |t, th| {
        // Spherical lens profile: compress rim, expand center.
        let r = t * (1.0 - amt * 0.45 * (1.0 - t * t));
        (r.max(0.0), th)
    });
}

fn k_lens_distortion(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amt, zoom) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0);
    if amt.abs() < 0.001 && (zoom - 1.0).abs() < 0.001 {
        return;
    }
    remap(buf, |x, y, w, h| {
        let (cx, cy) = (w * 0.5, h * 0.5);
        let m = w.min(h) * 0.5;
        let (nx, ny) = ((x - cx) / m, (y - cy) / m);
        let r2 = nx * nx + ny * ny;
        // Inverse-map with one Newton step of the forward barrel model.
        let s = 1.0 / (1.0 + amt * r2).max(0.2) / zoom.max(0.5);
        (cx + nx * s * m, cy + ny * s * m)
    });
}

fn k_chromatic(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amt, ang) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1));
    if amt < 0.01 {
        return;
    }
    let rad = ang.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            // Radial-scaled lateral shift: stronger toward the edges.
            let (w, h) = (buf.w as f32, buf.h as f32);
            let ex = ((x as f32 - w * 0.5) / w).abs() * 2.0;
            let ey = ((y as f32 - h * 0.5) / h).abs() * 2.0;
            let e = (ex + ey) * 0.5;
            let o = amt * 0.15 * (0.25 + e);
            let r = src.sample(x as f32 + dx * o, y as f32 + dy * o);
            let b = src.sample(x as f32 - dx * o, y as f32 - dy * o);
            buf.px[idx] = Px { r: r.r, g: d.g, b: b.b, a: d.a };
        }
    }
}

fn k_mesh_warp(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (dens, warp, rip) =
        (stock_p(plugin, p, 0).round().clamp(2.0, 8.0), stock_p(plugin, p, 1), stock_p(plugin, p, 2));
    if warp < 0.05 && rip < 0.05 {
        return;
    }
    let n = dens as usize;
    remap(buf, |x, y, w, h| {
        let gx = x / w * n as f32;
        let gy = y / h * n as f32;
        // Cell-corner pseudo-random offsets + sinusoidal ripple.
        let jx = (vnoise(gx.floor(), gy.floor(), 3.0) - 0.5) * warp * 0.02 * w.min(h);
        let jy = (vnoise(gx.floor(), gy.floor(), 9.0) - 0.5) * warp * 0.02 * w.min(h);
        let rx = (x * 0.05).sin() * (y * 0.05).cos() * rip * 0.02 * w.min(h);
        let ry = (y * 0.05).sin() * (x * 0.05).cos() * rip * 0.02 * w.min(h);
        (x - jx - rx, y - jy - ry)
    });
}

fn k_liquify(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (px, py, rad, stren, tw) = (
        stock_p(plugin, p, 0) / 100.0,
        stock_p(plugin, p, 1) / 100.0,
        stock_p(plugin, p, 2),
        stock_p(plugin, p, 3) / 100.0,
        stock_p(plugin, p, 4).to_radians(),
    );
    if stren.abs() < 0.001 && tw.abs() < 0.001 {
        return;
    }
    remap(buf, |x, y, w, h| {
        let (cx, cy) = (w * (0.5 + px * 0.5), h * (0.5 + py * 0.5));
        let m = w.min(h) * 0.5;
        let r_max = (rad / 100.0).clamp(0.02, 1.0) * m;
        let dx = x - cx;
        let dy = y - cy;
        let r = (dx * dx + dy * dy).sqrt();
        if r > r_max || r < 1e-4 {
            return (x, y);
        }
        let fall = 1.0 - r / r_max;
        let push = stren * fall * fall * m * 0.4;
        let th = dy.atan2(dx) + tw * fall;
        // Push outward along the (twirled) radius.
        let nr = (r - push).max(0.0);
        (cx + nr * th.cos(), cy + nr * th.sin())
    });
}

// ---------------------------------------------------------------------------
// Stylize kernels
// ---------------------------------------------------------------------------

/// Sobel edge magnitude in [0, 1] on straight luma.
pub(crate) fn sobel(src: &FloatBuf, x: u32, y: u32) -> f32 {
    let l = |dx: i32, dy: i32| -> f32 {
        let sx = (x as i32 + dx).clamp(0, src.w as i32 - 1);
        let sy = (y as i32 + dy).clamp(0, src.h as i32 - 1);
        pluma(&src.px[(sy as u32 * src.w + sx as u32) as usize])
    };
    let gx = -l(-1, -1) - 2.0 * l(-1, 0) - l(-1, 1) + l(1, -1) + 2.0 * l(1, 0) + l(1, 1);
    let gy = -l(-1, -1) - 2.0 * l(0, -1) - l(1, -1) + l(-1, 1) + 2.0 * l(0, 1) + l(1, 1);
    ((gx * gx + gy * gy).sqrt() * 0.35).clamp(0.0, 1.0)
}

fn k_edge(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (th, inv) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0);
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let a = src.px[idx].a;
            if a <= 0.0 {
                continue;
            }
            let mut e = sobel(&src, x, y);
            e = ((e - th) / 0.15f32.max(1e-3)).clamp(0.0, 1.0);
            e = e * (1.0 - inv) + (1.0 - e) * inv;
            buf.px[idx] = Px { r: e * a, g: e * a, b: e * a, a };
        }
    }
}

fn k_cartoon(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (levels, edge) = (
        stock_p(plugin, p, 0).round().clamp(2.0, 8.0),
        stock_p(plugin, p, 1) / 100.0,
    );
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let ia = 1.0 / d.a.max(1e-6);
            let q = |v: f32| ((v * (levels - 1.0)).round() / (levels - 1.0)).clamp(0.0, 1.0);
            let e = sobel(&src, x, y);
            let ink = (1.0 - (e * (0.5 + edge * 2.0)).clamp(0.0, 1.0)) * d.a;
            buf.px[idx] = Px { r: q(d.r * ia) * ink, g: q(d.g * ia) * ink, b: q(d.b * ia) * ink, a: d.a };
        }
    }
}

fn k_halftone(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (size, ang) = (stock_p(plugin, p, 0).clamp(2.0, 32.0), stock_p(plugin, p, 1).to_radians());
    let (ca, sa) = (ang.cos(), ang.sin());
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let l = pluma(&d);
            // Rotated dot screen: dot radius grows with darkness.
            let u = (x as f32 * ca - y as f32 * sa) / size;
            let v = (x as f32 * sa + y as f32 * ca) / size;
            let cell = ((u - u.floor() - 0.5).powi(2) + (v - v.floor() - 0.5).powi(2)).sqrt() * 2.0;
            let dot = if cell < (1.0 - l) * 1.1 { 1.0 } else { 0.06 };
            buf.px[idx] = Px { r: dot * d.a, g: dot * d.a, b: dot * d.a, a: d.a };
        }
    }
}

fn k_sketch(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (inten, inv) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0);
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let a = src.px[idx].a;
            if a <= 0.0 {
                continue;
            }
            let e = sobel(&src, x, y);
            let mut v = 1.0 - (e * (0.5 + inten * 3.0)).clamp(0.0, 1.0);
            v = v * (1.0 - inv) + (1.0 - v) * inv;
            buf.px[idx] = Px { r: v * a, g: v * a, b: v * a, a };
        }
    }
}

fn k_emboss(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (stren, ang) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1).to_radians());
    if stren <= 0.001 {
        return;
    }
    let (dx, dy) = (ang.cos(), ang.sin());
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let a = src.px[idx].a;
            if a <= 0.0 {
                continue;
            }
            let l = |ox: f32, oy: f32| pluma(&src.sample(x as f32 + ox, y as f32 + oy));
            let d = (l(dx, dy) - l(-dx, -dy)) * stren * 2.0;
            let v = (0.5 + d).clamp(0.0, 1.0);
            buf.px[idx] = Px { r: v * a, g: v * a, b: v * a, a };
        }
    }
}

fn k_pixelate(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, quantize: f32) {
    let size = stock_p(plugin, p, 0).clamp(1.0, 128.0).max(1.0);
    if size < 1.5 && quantize < 2.0 {
        return;
    }
    let src = snap(buf);
    let n = quantize.max(2.0);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let bx = ((x as f32 / size).floor() * size + size * 0.5).min(buf.w as f32 - 1.0);
            let by = ((y as f32 / size).floor() * size + size * 0.5).min(buf.h as f32 - 1.0);
            let mut s = src.sample(bx, by);
            if quantize >= 2.0 && s.a > 0.0 {
                let ia = 1.0 / s.a.max(1e-6);
                let q = |v: f32| ((v * (n - 1.0)).round() / (n - 1.0)).clamp(0.0, 1.0) * s.a;
                s = Px { r: q(s.r * ia), g: q(s.g * ia), b: q(s.b * ia), a: s.a };
            }
            buf.px[idx] = s;
        }
    }
}

fn k_vhs(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (track, noise_amt, chroma) =
        (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0, stock_p(plugin, p, 2));
    if track <= 0.001 && noise_amt <= 0.001 && chroma < 0.05 {
        return;
    }
    let src = snap(buf);
    let seed = ctx.frame as f32 * 0.731 + 4.2;
    let roll = hash2(0.0, ctx.frame as f32, 7.0) * track;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let yn = y as f32 / buf.h.max(1) as f32;
            // Tracking band: horizontal slice shift near the roll line.
            let band = ((yn - roll).abs() < 0.03 + track * 0.1) as u8 as f32;
            let ox = band * (hash2(y as f32, ctx.frame as f32, 3.0) - 0.5) * buf.w as f32 * 0.2 * track;
            let r = src.sample(x as f32 + ox + chroma, y as f32);
            let b = src.sample(x as f32 + ox - chroma, y as f32);
            let g = src.sample(x as f32 + ox, y as f32);
            let n = (hash2(x as f32, y as f32, seed) - 0.5) * noise_amt;
            buf.px[idx] = Px {
                r: (r.r + n * d.a).clamp(0.0, 1.0),
                g: (g.g + n * d.a).clamp(0.0, 1.0),
                b: (b.b + n * d.a).clamp(0.0, 1.0),
                a: d.a,
            };
        }
    }
}

fn k_rgb_split(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (amt, ang) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1));
    if amt < 0.01 {
        return;
    }
    let rad = ang.to_radians();
    let (dx, dy) = (rad.cos(), rad.sin());
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let r = src.sample(x as f32 + dx * amt, y as f32 + dy * amt);
            let b = src.sample(x as f32 - dx * amt, y as f32 - dy * amt);
            buf.px[idx] = Px { r: r.r, g: d.g, b: b.b, a: d.a };
        }
    }
}

fn k_scanlines(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (size, inten) = (stock_p(plugin, p, 0).clamp(1.0, 16.0), stock_p(plugin, p, 1) / 100.0);
    if inten <= 0.001 {
        return;
    }
    for y in 0..buf.h {
        let m = if (y as f32 / size).floor() % 2.0 < 1.0 { 1.0 } else { 1.0 - inten * 0.85 };
        for x in 0..buf.w {
            let px = &mut buf.px[(y * buf.w + x) as usize];
            px.r *= m;
            px.g *= m;
            px.b *= m;
        }
    }
}

fn k_glitch(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, seed) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1));
    if amt <= 0.001 {
        return;
    }
    let src = snap(buf);
    let t = ctx.frame as f32 + seed * 17.0;
    let bands = 4 + (amt * 10.0) as usize;
    for y in 0..buf.h {
        let band = (y as f32 / buf.h.max(1) as f32 * bands as f32).floor();
        let h = hash2(band, t.floor(), seed);
        let active = h < amt * 0.9;
        let ox = if active { (hash2(band, t.floor() + 0.5, seed + 1.0) - 0.5) * buf.w as f32 * 0.3 * amt } else { 0.0 };
        let shift_c = active && h < amt * 0.4;
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            if !active {
                continue;
            }
            let r = src.sample(x as f32 + ox + if shift_c { 6.0 } else { 0.0 }, y as f32);
            let b = src.sample(x as f32 + ox - if shift_c { 6.0 } else { 0.0 }, y as f32);
            let inv = hash2(band * 3.1, t.floor(), 9.0) < amt * 0.15;
            let (rr, gg, bb) = if inv { (1.0 - r.r, 1.0 - d.g, 1.0 - b.b) } else { (r.r, d.g, b.b) };
            buf.px[idx] = Px { r: rr.min(1.0), g: gg.min(1.0), b: bb.min(1.0), a: d.a };
        }
    }
}

// ---------------------------------------------------------------------------
// Noise / film kernels (time-seeded via ctx)
// ---------------------------------------------------------------------------

fn k_film_grain(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, size, mono) =
        (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1).max(1.0), stock_p(plugin, p, 2) / 100.0);
    if amt <= 0.001 {
        return;
    }
    let seed = ctx.frame as f32 * 0.913 + 2.0;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = buf.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let bx = (x as f32 / size).floor();
            let by = (y as f32 / size).floor();
            let h1 = hash2(bx, by, seed) - 0.5;
            let n = h1 * amt;
            if mono > 0.5 {
                buf.px[idx] = Px {
                    r: (d.r + n * d.a).clamp(0.0, 1.0),
                    g: (d.g + n * d.a).clamp(0.0, 1.0),
                    b: (d.b + n * d.a).clamp(0.0, 1.0),
                    a: d.a,
                };
            } else {
                let h2 = hash2(bx + 40.0, by, seed + 1.0) - 0.5;
                let h3 = hash2(bx, by + 40.0, seed + 2.0) - 0.5;
                buf.px[idx] = Px {
                    r: (d.r + n * d.a).clamp(0.0, 1.0),
                    g: (d.g + h2 * amt * d.a).clamp(0.0, 1.0),
                    b: (d.b + h3 * amt * d.a).clamp(0.0, 1.0),
                    a: d.a,
                };
            }
        }
    }
}

fn k_fractal_noise(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx, fill: bool, col_a: Color, col_b: Color) {
    let (scale, oct, seed, evo) = (
        stock_p(plugin, p, 0).clamp(0.1, 10.0),
        stock_p(plugin, p, 1).round().clamp(1.0, 8.0) as usize,
        stock_p(plugin, p, 2),
        stock_p(plugin, p, 3),
    );
    let t = ctx.time_s * evo * 0.1;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = buf.px[idx];
            if !fill && d.a <= 0.0 {
                continue;
            }
            let n = fbm(x as f32 / buf.w.max(1) as f32 * 8.0 * scale + t, y as f32 / buf.h.max(1) as f32 * 8.0 * scale, oct, seed);
            if fill {
                buf.px[idx] = Px {
                    r: (col_a.r + (col_b.r - col_a.r) * n),
                    g: (col_a.g + (col_b.g - col_a.g) * n),
                    b: (col_a.b + (col_b.b - col_a.b) * n),
                    a: 1.0,
                };
            } else {
                // Blend fractal detail over content (turbulence-style).
                let m = (n - 0.5) * d.a;
                buf.px[idx] = Px {
                    r: (d.r + m * 0.6).clamp(0.0, 1.0),
                    g: (d.g + m * 0.6).clamp(0.0, 1.0),
                    b: (d.b + m * 0.6).clamp(0.0, 1.0),
                    a: d.a,
                };
            }
        }
    }
}

fn k_dust(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, size, seed) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1).max(1.0), stock_p(plugin, p, 2));
    if amt <= 0.001 {
        return;
    }
    let t = ctx.frame as f32;
    // Sparse cells become dark or bright specks.
    let cell = 9.0;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = buf.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            let cx = (x as f32 / cell).floor();
            let cy = (y as f32 / cell).floor();
            let h = hash2(cx, cy, seed + t * 0.37);
            if h > 1.0 - amt * 0.12 {
                let lx = x as f32 - cx * cell - cell * 0.5;
                let ly = y as f32 - cy * cell - cell * 0.5;
                if lx * lx + ly * ly < size * size * 0.25 {
                    let dark = hash2(cx + 7.0, cy, seed) > 0.5;
                    let v = if dark { 0.05 } else { 0.95 };
                    buf.px[idx] = Px { r: v * d.a, g: v * d.a, b: v * d.a, a: d.a };
                }
            }
        }
    }
}

fn k_scratches(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, len, seed) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0, stock_p(plugin, p, 2));
    if amt <= 0.001 {
        return;
    }
    let t = ctx.frame as f32;
    for x in 0..buf.w {
        let h = hash2(x as f32, t * 0.53, seed);
        if h > 1.0 - amt * 0.10 {
            let y0 = hash2(x as f32, 3.0, seed + 1.0) * buf.h.max(1) as f32;
            let lh = (len * buf.h as f32).max(4.0);
            let bright = hash2(x as f32, 9.0, seed + 2.0) > 0.4;
            for y in 0..buf.h {
                let dy = ((y as f32 - y0) % buf.h.max(1) as f32 + buf.h.max(1) as f32) % buf.h.max(1) as f32;
                if dy < lh {
                    let idx = (y * buf.w + x) as usize;
                    let d = buf.px[idx];
                    if d.a <= 0.0 {
                        continue;
                    }
                    let v = if bright { 0.9 } else { 0.08 };
                    let m = 0.75;
                    buf.px[idx] = Px {
                        r: (d.r * (1.0 - m) + v * d.a * m).min(1.0),
                        g: (d.g * (1.0 - m) + v * d.a * m).min(1.0),
                        b: (d.b * (1.0 - m) + v * d.a * m).min(1.0),
                        a: d.a,
                    };
                }
            }
        }
    }
}

fn k_film_damage(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, seed) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1));
    if amt <= 0.01 {
        return;
    }
    // Combo pass: dust + scratches + flicker, scaled by amount.
    k_dust(buf, &[amt * 0.8, 2.5, seed], plugin, ctx);
    k_scratches(buf, &[amt * 0.7, 35.0, seed + 3.0], plugin, ctx);
    // Flicker gain.
    let fl = (hash2(ctx.frame as f32, 1.0, seed + 5.0) - 0.5) * amt / 100.0 * 0.5;
    for px in buf.px.iter_mut() {
        px.r = (px.r * (1.0 + fl)).clamp(0.0, 1.0);
        px.g = (px.g * (1.0 + fl)).clamp(0.0, 1.0);
        px.b = (px.b * (1.0 + fl)).clamp(0.0, 1.0);
    }
}

fn k_flicker(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, speed) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0);
    if amt <= 0.001 {
        return;
    }
    let t = ctx.time_s * (0.5 + speed * 2.0);
    let g = 1.0 + (hash2(t.floor(), 0.5, 1.0) - 0.5) * 2.0 * amt * 0.6;
    for px in buf.px.iter_mut() {
        px.r = (px.r * g).clamp(0.0, 1.0);
        px.g = (px.g * g).clamp(0.0, 1.0);
        px.b = (px.b * g).clamp(0.0, 1.0);
    }
}

fn k_gate_weave(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, ctx: &RasterFx) {
    let (amt, speed) = (stock_p(plugin, p, 0), stock_p(plugin, p, 1));
    if amt <= 0.01 {
        return;
    }
    let t = ctx.time_s * (0.5 + speed * 2.0);
    let jx = (hash2(t.floor(), 2.0, 4.0) - 0.5) * amt * 0.06 * buf.w.max(1) as f32;
    let jy = (hash2(8.0, t.floor(), 4.0) - 0.5) * amt * 0.06 * buf.h.max(1) as f32;
    if jx.abs() < 0.05 && jy.abs() < 0.05 {
        return;
    }
    remap(buf, move |x, y, _, _| (x - jx, y - jy));
}

// ---------------------------------------------------------------------------
// Keying / matte kernels (alpha-channel morphology)
// ---------------------------------------------------------------------------

/// Min/max filter on straight alpha (erode = min, dilate = max).
fn morph_alpha(buf: &mut FloatBuf, radius: f32, dilate: bool) {
    let r = radius.clamp(0.0, 40.0) as i32;
    if r < 1 {
        return;
    }
    let src = snap(buf);
    // Straight-alpha field for clean min/max.
    let field: Vec<f32> = src.px.iter().map(|p| p.a).collect();
    let at = |x: i32, y: i32| -> f32 {
        let x = x.clamp(0, buf.w as i32 - 1) as usize;
        let y = y.clamp(0, buf.h as i32 - 1) as usize;
        field[y * buf.w as usize + x]
    };
    for y in 0..buf.h {
        for x in 0..buf.w {
            let mut v = if dilate { 0.0f32 } else { 1.0f32 };
            for dy in -r..=r {
                for dx in -r..=r {
                    let a = at(x as i32 + dx, y as i32 + dy);
                    v = if dilate { v.max(a) } else { v.min(a) };
                }
            }
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 && v <= 0.0 {
                continue;
            }
            // Rescale stored color to the new alpha (preserve straight rgb).
            let ia = 1.0 / d.a.max(1e-6);
            buf.px[idx] = Px { r: (d.r * ia).min(1.0) * v, g: (d.g * ia).min(1.0) * v, b: (d.b * ia).min(1.0) * v, a: v };
        }
    }
}

/// Blur straight alpha only (matte softening without touching color).
fn blur_alpha(buf: &mut FloatBuf, radius: f32) {
    let r = radius.clamp(0.0, 40.0);
    if r < 0.5 {
        return;
    }
    let mut a = FloatBuf::clear(buf.w, buf.h);
    for (d, s) in a.px.iter_mut().zip(buf.px.iter()) {
        *d = Px { r: s.a, g: s.a, b: s.a, a: 1.0 };
    }
    blur_buffer(&mut a, r);
    for (d, s) in buf.px.iter_mut().zip(a.px.iter()) {
        if d.a <= 0.0 && s.r <= 0.0 {
            continue;
        }
        let ia = 1.0 / d.a.max(1e-6);
        let na = s.r.clamp(0.0, 1.0);
        *d = Px { r: (d.r * ia).min(1.0) * na, g: (d.g * ia).min(1.0) * na, b: (d.b * ia).min(1.0) * na, a: na };
    }
}

fn k_matte_choker(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let choke = stock_p(plugin, p, 0);
    if choke.abs() < 0.05 {
        return;
    }
    if choke > 0.0 {
        morph_alpha(buf, choke * 0.4, false);
    } else {
        morph_alpha(buf, -choke * 0.4, true);
    }
}

fn k_key_cleaner(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (despill, choke) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1));
    // Despill greens toward the rb average.
    if despill > 0.001 {
        for px in buf.px.iter_mut() {
            if px.a <= 0.0 {
                continue;
            }
            let cap = (px.r + px.b) * 0.5;
            px.g = px.g * (1.0 - despill) + cap.min(px.g) * despill;
        }
    }
    if choke.abs() >= 0.05 {
        if choke > 0.0 {
            morph_alpha(buf, choke * 0.4, false);
        } else {
            morph_alpha(buf, -choke * 0.4, true);
        }
    }
}

// ---------------------------------------------------------------------------
// Transform / spatial kernels (buffer-space remaps; TransformFx folds into
// the blit map instead — see rasterize_layer / rasterize_comp)
// ---------------------------------------------------------------------------

fn k_crop(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (l, t, r, b) = (
        stock_p(plugin, p, 0) / 100.0,
        stock_p(plugin, p, 1) / 100.0,
        stock_p(plugin, p, 2) / 100.0,
        stock_p(plugin, p, 3) / 100.0,
    );
    if l <= 0.0 && t <= 0.0 && r <= 0.0 && b <= 0.0 {
        return;
    }
    let (w, h) = (buf.w as f32, buf.h as f32);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let fx = x as f32 / w.max(1.0);
            let fy = y as f32 / h.max(1.0);
            if fx < l || fy < t || fx > 1.0 - r || fy > 1.0 - b {
                buf.px[(y * buf.w + x) as usize] = Px::clear();
            }
        }
    }
}

/// Solve the projective map from unit-square corners to `dst` quad corners
/// (8x8 Gaussian elimination). Returns row-major homography or None when
/// the quad is degenerate.
#[allow(clippy::needless_range_loop)]
fn homography(dst: [(f32, f32); 4]) -> Option<[f32; 9]> {
    // Source corners in order: UL(0,0) UR(1,0) LR(1,1) LL(0,1).
    let src = [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let mut m = [[0.0f32; 9]; 8];
    for (i, ((sx, sy), (dx, dy))) in src.iter().zip(dst.iter()).enumerate() {
        m[i * 2] = [*sx, *sy, 1.0, 0.0, 0.0, 0.0, -dx * sx, -dx * sy, *dx];
        m[i * 2 + 1] = [0.0, 0.0, 0.0, *sx, *sy, 1.0, -dy * sx, -dy * sy, *dy];
    }
    // Forward elimination with partial pivoting.
    for col in 0..8 {
        let mut piv = col;
        for row in col..8 {
            if m[row][col].abs() > m[piv][col].abs() {
                piv = row;
            }
        }
        if m[piv][col].abs() < 1e-9 {
            return None;
        }
        m.swap(col, piv);
        for row in (col + 1)..8 {
            let f = m[row][col] / m[col][col];
            for k in col..9 {
                m[row][k] -= f * m[col][k];
            }
        }
    }
    let mut h = [0.0f32; 8];
    for i in (0..8).rev() {
        let mut s = m[i][8];
        for k in (i + 1)..8 {
            s -= m[i][k] * h[k];
        }
        h[i] = s / m[i][i];
    }
    Some([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0])
}

fn k_corner_pin(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let q = |i: usize| stock_p(plugin, p, i).clamp(0.0, 1.0);
    let quad = [(q(0), q(1)), (q(2), q(3)), (q(4), q(5)), (q(6), q(7))];
    // Identity quad -> pass through.
    let ident = quad[0] == (0.0, 0.0) && quad[1] == (1.0, 0.0) && quad[2] == (1.0, 1.0) && quad[3] == (0.0, 1.0);
    if ident {
        return;
    }
    // Map OUTPUT unit coords back to SOURCE unit coords: invert the pin.
    // H maps src-unit -> dst-unit; invert once, then per-pixel.
    let h = match homography(quad) {
        Some(h) => h,
        None => return,
    };
    let det = h[0] * (h[4] - h[5] * h[7]) - h[1] * (h[3] - h[5] * h[6]) + h[2] * (h[3] * h[7] - h[4] * h[6]);
    if det.abs() < 1e-9 {
        return;
    }
    let id = 1.0 / det;
    let inv = [
        (h[4] - h[5] * h[7]) * id,
        (h[2] * h[7] - h[1]) * id,
        (h[1] * h[5] - h[2] * h[4]) * id,
        (h[5] * h[6] - h[3]) * id,
        (h[0] - h[2] * h[6]) * id,
        (h[2] * h[3] - h[0] * h[5]) * id,
        (h[3] * h[7] - h[4] * h[6]) * id,
        (h[1] * h[6] - h[0] * h[7]) * id,
        (h[0] * h[4] - h[1] * h[3]) * id,
    ];
    remap(buf, |x, y, w, hgt| {
        let (ux, uy) = (x / w.max(1.0), y / hgt.max(1.0));
        let ww = inv[6] * ux + inv[7] * uy + inv[8];
        if ww.abs() < 1e-6 {
            return (-10.0, -10.0);
        }
        let sx = (inv[0] * ux + inv[1] * uy + inv[2]) / ww * w;
        let sy = (inv[3] * ux + inv[4] * uy + inv[5]) / ww * hgt;
        (sx, sy)
    });
}

/// Card 3D: rotate the frame rectangle about its pivot in 3D (rx then ry)
/// and project with a true perspective divide, reusing the corner-pin
/// homography machinery. Aspect-corrected so rotation is circular on any
/// frame. Backface (flipped winding) culls to transparent when enabled.
fn k_card_3d(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    if buf.w == 0 || buf.h == 0 {
        return;
    }
    let vals = compositor::fx::stock_params_resolved(plugin, p);
    let (w, h) = (buf.w as f32, buf.h as f32);
    match compositor::fx::card_3d_plan(&vals, w, h) {
        compositor::fx::Card3dPlan::Identity | compositor::fx::Card3dPlan::Keep => {}
        compositor::fx::Card3dPlan::Clear => {
            for px in buf.px.iter_mut() {
                *px = Px::clear();
            }
        }
        compositor::fx::Card3dPlan::Project(inv) => {
            remap(buf, |x, y, w, hgt| {
                let (ux, uy) = (x / w.max(1.0), y / hgt.max(1.0));
                let ww = inv[6] * ux + inv[7] * uy + inv[8];
                if ww.abs() < 1e-6 {
                    return (-10.0, -10.0);
                }
                let sx = (inv[0] * ux + inv[1] * uy + inv[2]) / ww * w;
                let sy = (inv[3] * ux + inv[4] * uy + inv[5]) / ww * hgt;
                (sx, sy)
            });
        }
    }
}

fn k_mirror(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (mode, center) = (stock_p(plugin, p, 0).round() as i32, stock_p(plugin, p, 1) / 100.0);
    if !(0..=2).contains(&mode) {
        return;
    }
    remap(buf, |x, y, w, h| {
        let cx = w * center;
        let cy = h * center;
        let nx = if mode == 0 || mode == 2 { 2.0 * cx - x } else { x };
        let ny = if mode == 1 || mode == 2 { 2.0 * cy - y } else { y };
        (nx, ny)
    });
}

fn k_repeat(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (tx, ty) = (stock_p(plugin, p, 0).round().max(1.0), stock_p(plugin, p, 1).round().max(1.0));
    if tx <= 1.0 && ty <= 1.0 {
        return;
    }
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let sx = (((x as f32 * tx / buf.w.max(1) as f32) % 1.0 + 1.0) % 1.0 * buf.w as f32) as i32;
            let sy = (((y as f32 * ty / buf.h.max(1) as f32) % 1.0 + 1.0) % 1.0 * buf.h as f32) as i32;
            buf.px[(y * buf.w + x) as usize] =
                src.px[(sy.clamp(0, buf.h as i32 - 1) as u32 * buf.w + sx.clamp(0, buf.w as i32 - 1) as u32) as usize];
        }
    }
}

fn k_offset(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (sx, sy) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1) / 100.0);
    if sx.abs() < 0.0005 && sy.abs() < 0.0005 {
        return;
    }
    remap(buf, move |x, y, w, h| {
        // Wrap-around offset (fractions of the frame).
        let nx = (x - sx * w).rem_euclid(w);
        let ny = (y - sy * h).rem_euclid(h);
        (nx, ny)
    });
}

fn k_reframe(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (scale, ox, oy) = (
        stock_p(plugin, p, 0) / 100.0,
        stock_p(plugin, p, 1) / 100.0,
        stock_p(plugin, p, 2) / 100.0,
    );
    if (scale - 1.0).abs() < 0.001 && ox.abs() < 0.001 && oy.abs() < 0.001 {
        return;
    }
    remap(buf, move |x, y, w, h| {
        let (cx, cy) = (w * (0.5 + ox * 0.5), h * (0.5 + oy * 0.5));
        (cx + (x - w * 0.5) / scale.max(0.1), cy + (y - h * 0.5) / scale.max(0.1))
    });
}

// ---------------------------------------------------------------------------
// Cleanup kernels
// ---------------------------------------------------------------------------

fn k_denoise(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (rad, stren) = (stock_p(plugin, p, 0).clamp(0.0, 10.0), stock_p(plugin, p, 1) / 100.0);
    if rad < 0.5 || stren <= 0.01 {
        return;
    }
    let mut soft = snap(buf);
    box_blur(&mut soft, rad);
    for (d, s) in buf.px.iter_mut().zip(soft.px.iter()) {
        if d.a <= 0.0 {
            continue;
        }
        d.r = (d.r * (1.0 - stren) + s.r * stren).min(1.0);
        d.g = (d.g * (1.0 - stren) + s.g * stren).min(1.0);
        d.b = (d.b * (1.0 - stren) + s.b * stren).min(1.0);
    }
}

fn k_deband(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (rad, th) = (stock_p(plugin, p, 0).clamp(0.0, 20.0), stock_p(plugin, p, 1) / 100.0);
    if rad < 0.5 {
        return;
    }
    let mut soft = snap(buf);
    box_blur(&mut soft, rad);
    // Only blend where the neighbourhood agrees (smooth gradients);
    // detail stronger than `threshold` is preserved.
    for (d, s) in buf.px.iter_mut().zip(soft.px.iter()) {
        if d.a <= 0.0 {
            continue;
        }
        let diff = ((d.r - s.r).abs() + (d.g - s.g).abs() + (d.b - s.b).abs()) / 3.0;
        let m = ((th - diff) / th.max(1e-3)).clamp(0.0, 1.0);
        d.r = d.r * (1.0 - m) + s.r * m;
        d.g = d.g * (1.0 - m) + s.g * m;
        d.b = d.b * (1.0 - m) + s.b * m;
    }
}

fn k_degrain(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    // Stronger denoise: bilateral-lite via two small range-weighted passes.
    let amt = (stock_p(plugin, p, 0) / 100.0).clamp(0.0, 1.0);
    if amt <= 0.01 {
        return;
    }
    let r = 2i32;
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let c = src.px[(y * buf.w + x) as usize];
            let mut acc = Px { r: c.r, g: c.g, b: c.b, a: c.a };
            let mut n = 1.0;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (sx, sy) = (x as i32 + dx, y as i32 + dy);
                    if sx < 0 || sy < 0 || sx >= buf.w as i32 || sy >= buf.h as i32 {
                        continue;
                    }
                    let s = src.px[(sy as u32 * buf.w + sx as u32) as usize];
                    let dr = (pluma(&c) - pluma(&s)).abs();
                    let w = (-dr * 8.0).exp();
                    acc.r += s.r * w;
                    acc.g += s.g * w;
                    acc.b += s.b * w;
                    acc.a += s.a * w;
                    n += w;
                }
            }
            let idx = (y * buf.w + x) as usize;
            buf.px[idx] = Px {
                r: (c.r * (1.0 - amt) + acc.r / n * amt).min(1.0),
                g: (c.g * (1.0 - amt) + acc.g / n * amt).min(1.0),
                b: (c.b * (1.0 - amt) + acc.b / n * amt).min(1.0),
                a: (c.a * (1.0 - amt) + acc.a / n * amt).min(1.0),
            };
        }
    }
}

fn k_deblur(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    // Deconvolution-lite: strong unsharp mask.
    super::effects::apply_sharpen(buf, stock_p(plugin, p, 0), stock_p(plugin, p, 1).max(0.5));
}

fn k_dust_removal(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (th, rad) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1).clamp(0.0, 10.0).max(1.0));
    if th <= 0.001 {
        return;
    }
    let mut avg = snap(buf);
    box_blur(&mut avg, rad);
    for (d, s) in buf.px.iter_mut().zip(avg.px.iter()) {
        if d.a <= 0.0 {
            continue;
        }
        let diff = ((d.r - s.r).abs() + (d.g - s.g).abs() + (d.b - s.b).abs()) / 3.0;
        if diff > th {
            *d = *s;
        }
    }
}

fn k_scratch_removal(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let (th, width) = (stock_p(plugin, p, 0) / 100.0, stock_p(plugin, p, 1).clamp(1.0, 12.0) as i32);
    if th <= 0.001 {
        return;
    }
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            // Vertical anomaly: differs from BOTH horizontal neighbours.
            let l = src.sample(x as f32 - width as f32, y as f32);
            let r = src.sample(x as f32 + width as f32, y as f32);
            let dl = ((d.r - l.r).abs() + (d.g - l.g).abs() + (d.b - l.b).abs()) / 3.0;
            let dr = ((d.r - r.r).abs() + (d.g - r.g).abs() + (d.b - r.b).abs()) / 3.0;
            if dl > th && dr > th {
                buf.px[idx] = Px {
                    r: (l.r + r.r) * 0.5,
                    g: (l.g + r.g) * 0.5,
                    b: (l.b + r.b) * 0.5,
                    a: d.a,
                };
            }
        }
    }
}

fn k_dead_pixel(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin) {
    let th = (stock_p(plugin, p, 0) / 100.0).clamp(0.0, 1.0);
    if th <= 0.001 {
        return;
    }
    let src = snap(buf);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let idx = (y * buf.w + x) as usize;
            let d = src.px[idx];
            if d.a <= 0.0 {
                continue;
            }
            // 4-neighbour median-ish: outlier vs all four sides.
            let n4 = [
                src.sample(x as f32 - 1.0, y as f32),
                src.sample(x as f32 + 1.0, y as f32),
                src.sample(x as f32, y as f32 - 1.0),
                src.sample(x as f32, y as f32 + 1.0),
            ];
            let dl = pluma(&d);
            let mut votes = 0;
            let mut acc = Px::clear();
            for n in &n4 {
                if (dl - pluma(n)).abs() > th {
                    votes += 1;
                }
                acc.r += n.r * 0.25;
                acc.g += n.g * 0.25;
                acc.b += n.b * 0.25;
            }
            if votes == 4 {
                buf.px[idx] = Px { r: acc.r, g: acc.g, b: acc.b, a: d.a };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Generators (fill the frame)
// ---------------------------------------------------------------------------

fn stock_color(colors: &[Color], i: usize, fallback: Color) -> Color {
    colors.get(i).copied().unwrap_or(fallback)
}

fn k_solid(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let op = (stock_p(plugin, p, 0) / 100.0).clamp(0.0, 1.0);
    let c = stock_color(colors, 0, Color::WHITE);
    for px in buf.px.iter_mut() {
        *px = Px { r: c.r * op, g: c.g * op, b: c.b * op, a: op };
    }
}

fn k_fractal_gen(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color], ctx: &RasterFx) {
    let (scale, oct, seed, op) = (
        stock_p(plugin, p, 0).clamp(0.1, 10.0),
        stock_p(plugin, p, 1).round().clamp(1.0, 8.0) as usize,
        stock_p(plugin, p, 2),
        (stock_p(plugin, p, 3) / 100.0).clamp(0.0, 1.0),
    );
    let (ca, cb) = (stock_color(colors, 0, Color::BLACK), stock_color(colors, 1, Color::rgb(0.2, 0.5, 1.0)));
    let t = ctx.time_s * 0.05;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let n = fbm(x as f32 / buf.w.max(1) as f32 * 6.0 * scale + t, y as f32 / buf.h.max(1) as f32 * 6.0 * scale, oct, seed);
            buf.px[(y * buf.w + x) as usize] = Px {
                r: (ca.r + (cb.r - ca.r) * n) * op,
                g: (ca.g + (cb.g - ca.g) * n) * op,
                b: (ca.b + (cb.b - ca.b) * n) * op,
                a: op,
            };
        }
    }
}

fn k_grid(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let (size, line, op) = (
        stock_p(plugin, p, 0).clamp(2.0, 256.0),
        stock_p(plugin, p, 1).clamp(1.0, 32.0),
        (stock_p(plugin, p, 2) / 100.0).clamp(0.0, 1.0),
    );
    let c = stock_color(colors, 0, Color::WHITE);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let fx = (x as f32 % size).min(size - x as f32 % size);
            let fy = (y as f32 % size).min(size - y as f32 % size);
            if fx < line || fy < line {
                buf.px[(y * buf.w + x) as usize] = Px { r: c.r * op, g: c.g * op, b: c.b * op, a: op };
            } else {
                buf.px[(y * buf.w + x) as usize] = Px::clear();
            }
        }
    }
}

fn k_shapes(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color]) {
    let (shape, size, soft, op) = (
        stock_p(plugin, p, 0).round() as i32,
        stock_p(plugin, p, 1).clamp(0.0, 100.0) / 100.0,
        stock_p(plugin, p, 2).clamp(0.0, 100.0) / 100.0,
        (stock_p(plugin, p, 3) / 100.0).clamp(0.0, 1.0),
    );
    let c = stock_color(colors, 0, Color::rgb(0.25, 0.6, 1.0));
    let (w, h) = (buf.w as f32, buf.h as f32);
    let r = size * w.min(h) * 0.5;
    let feather = (soft * r * 0.5 + 0.5).max(0.5);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let dx = (x as f32 - w * 0.5).abs();
            let dy = (y as f32 - h * 0.5).abs();
            let d = match shape {
                1 => dx.max(dy) - r, // rect (SDF-ish)
                2 => ((dx * dx + dy * dy).sqrt() - r * 0.6).abs() - r * 0.18, // ring
                _ => (dx * dx + dy * dy).sqrt() - r, // circle
            };
            let a = ((feather - d) / (2.0 * feather)).clamp(0.0, 1.0) * op;
            buf.px[(y * buf.w + x) as usize] = Px { r: c.r * a, g: c.g * a, b: c.b * a, a };
        }
    }
}

fn k_plasma(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color], ctx: &RasterFx) {
    let (scale, speed, op) = (
        stock_p(plugin, p, 0).clamp(0.1, 10.0),
        stock_p(plugin, p, 1),
        (stock_p(plugin, p, 2) / 100.0).clamp(0.0, 1.0),
    );
    let (ca, cb) = (stock_color(colors, 0, Color::rgb(0.05, 0.1, 0.4)), stock_color(colors, 1, Color::rgb(0.2, 0.9, 1.0)));
    let t = ctx.time_s * speed * 0.05;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let nx = x as f32 / buf.w.max(1) as f32 * 6.0 * scale;
            let ny = y as f32 / buf.h.max(1) as f32 * 6.0 * scale;
            let v = (fbm(nx + t, ny, 4, 1.0) * 0.6 + fbm(nx * 1.7 - t, ny * 1.7, 4, 7.0) * 0.4).clamp(0.0, 1.0);
            // Cosine palette between the two colors.
            let m = 0.5 + 0.5 * (v * std::f32::consts::TAU).cos();
            buf.px[(y * buf.w + x) as usize] = Px {
                r: (ca.r + (cb.r - ca.r) * m) * op,
                g: (ca.g + (cb.g - ca.g) * m) * op,
                b: (ca.b + (cb.b - ca.b) * m) * op,
                a: op,
            };
        }
    }
}

fn k_particles(buf: &mut FloatBuf, p: &[f32], plugin: StockPlugin, colors: &[Color], ctx: &RasterFx) {
    let (count, size, speed) = (
        stock_p(plugin, p, 0).round().clamp(1.0, 500.0) as usize,
        stock_p(plugin, p, 1).clamp(1.0, 16.0),
        stock_p(plugin, p, 2),
    );
    let c = stock_color(colors, 0, Color::rgb(0.25, 0.6, 1.0));
    for px in buf.px.iter_mut() {
        *px = Px::clear();
    }
    let t = ctx.time_s * speed * 0.05;
    let (w, h) = (buf.w as f32, buf.h as f32);
    for i in 0..count {
        let fi = i as f32;
        let px = (hash2(fi, 1.0, 3.0) + t * (0.02 + hash2(fi, 2.0, 5.0) * 0.08)) % 1.0 * w;
        let py = (hash2(fi, 7.0, 3.0) + t * (0.01 + hash2(fi, 8.0, 5.0) * 0.05)) % 1.0 * h;
        let r = size * (0.5 + hash2(fi, 9.0, 3.0));
        let tw = 0.5 + 0.5 * (ctx.time_s * 2.0 + fi).sin();
        let x0 = (px - r) as i32;
        let x1 = (px + r) as i32;
        let y0 = (py - r) as i32;
        let y1 = (py + r) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x < 0 || y < 0 || x >= buf.w as i32 || y >= buf.h as i32 {
                    continue;
                }
                let d = ((x as f32 - px).powi(2) + (y as f32 - py).powi(2)).sqrt() / r.max(0.5);
                if d > 1.0 {
                    continue;
                }
                let a = ((1.0 - d) * (0.4 + 0.6 * tw)).clamp(0.0, 1.0);
                let idx = (y as u32 * buf.w + x as u32) as usize;
                let o = buf.px[idx];
                buf.px[idx] = Px {
                    r: (o.r + c.r * a * (1.0 - o.a)).min(1.0),
                    g: (o.g + c.g * a * (1.0 - o.a)).min(1.0),
                    b: (o.b + c.b * a * (1.0 - o.a)).min(1.0),
                    a: (o.a + a * (1.0 - o.a)).min(1.0),
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Apply one evaluated stock plug-in. `TransformFx` is a no-op here: it
/// folds into the blit map (see `fold_stock_transform`).
pub fn apply_stock(
    buf: &mut FloatBuf,
    plugin: StockPlugin,
    params: &[f32],
    colors: &[Color],
    ctx: &RasterFx,
) {
    use StockPlugin as S;
    match plugin {
        // Per-pixel kernels resolve through process_color (shared with
        // adjustment layers and headless evaluation).
        S::Curves | S::ColorBalance | S::ColorWheels | S::TemperatureTint | S::Posterize
        | S::Threshold | S::DifferenceKey | S::SpillSuppress => {
            // Resolve descriptor defaults once per layer (not per pixel).
            let vals = compositor::fx::stock_params_resolved(plugin, params);
            for px in buf.px.iter_mut() {
                if px.a <= 0.0 {
                    continue;
                }
                *px = Px::from_color(compositor::fx::process_color_stock_resolved(plugin, &vals, px.to_color()));
            }
        }
        // Blur
        S::BoxBlur => k_box_blur(buf, params, plugin),
        S::DirectionalBlur | S::MotionBlur => k_directional(buf, params, plugin),
        S::RadialBlur => k_radial(buf, params, plugin, false),
        S::ZoomBlur => k_radial(buf, params, plugin, true),
        S::Defocus => k_defocus(buf, params, plugin),
        S::Bokeh => k_bokeh(buf, params, plugin),
        S::Bilateral => k_bilateral(buf, params, plugin),
        // Light
        S::Glow => k_glow(buf, params, plugin),
        S::Glare => k_glare(buf, params, plugin),
        S::Glint => k_glint(buf, params, plugin),
        S::LightRays => k_light_rays(buf, params, plugin, colors),
        S::GodRays => k_god_rays(buf, params, plugin, colors),
        S::LensFlare => k_lens_flare(buf, params, plugin, colors),
        S::LightLeak => k_light_leak(buf, params, plugin),
        S::Streaks => k_streaks(buf, params, plugin),
        S::Halo => k_halo(buf, params, plugin),
        S::LongShadow => k_long_shadow(buf, params, plugin, colors),
        S::Saber => k_saber(buf, params, plugin, colors, ctx),
        // Distort
        S::TurbulentDisplace => k_turbulent(buf, params, plugin),
        S::Wave => k_wave(buf, params, plugin),
        S::Ripple => k_ripple(buf, params, plugin),
        S::Twirl => k_twirl(buf, params, plugin),
        S::Bulge => k_bulge(buf, params, plugin),
        S::Spherize => k_spherize(buf, params, plugin),
        S::LensDistortion => k_lens_distortion(buf, params, plugin),
        S::ChromaticAberration => k_chromatic(buf, params, plugin),
        S::MeshWarp => k_mesh_warp(buf, params, plugin),
        S::Liquify => k_liquify(buf, params, plugin),
        // Stylize
        S::EdgeDetect => k_edge(buf, params, plugin),
        S::Cartoon => k_cartoon(buf, params, plugin),
        S::Halftone => k_halftone(buf, params, plugin),
        S::Sketch => k_sketch(buf, params, plugin),
        S::Emboss => k_emboss(buf, params, plugin),
        S::Pixelate => k_pixelate(buf, params, plugin, 0.0),
        S::Mosaic => k_pixelate(buf, params, plugin, stock_p(plugin, params, 1)),
        S::Vhs => k_vhs(buf, params, plugin, ctx),
        S::RgbSplit => k_rgb_split(buf, params, plugin),
        S::Scanlines => k_scanlines(buf, params, plugin),
        S::Glitch => k_glitch(buf, params, plugin, ctx),
        // Noise / film
        S::FilmGrain => k_film_grain(buf, params, plugin, ctx),
        S::FractalNoise => k_fractal_noise(buf, params, plugin, ctx, false, Color::BLACK, Color::WHITE),
        S::Turbulence => k_fractal_noise(buf, params, plugin, ctx, false, Color::BLACK, Color::WHITE),
        S::Dust => k_dust(buf, params, plugin, ctx),
        S::Scratches => k_scratches(buf, params, plugin, ctx),
        S::FilmDamage => k_film_damage(buf, params, plugin, ctx),
        S::Flicker => k_flicker(buf, params, plugin, ctx),
        S::GateWeave => k_gate_weave(buf, params, plugin, ctx),
        // Key / matte
        S::MatteChoker => k_matte_choker(buf, params, plugin),
        S::MatteBlur => blur_alpha(buf, stock_p(plugin, params, 0)),
        S::Erode => morph_alpha(buf, stock_p(plugin, params, 0) * 0.4, false),
        S::Dilate => morph_alpha(buf, stock_p(plugin, params, 0) * 0.4, true),
        S::KeyCleaner => k_key_cleaner(buf, params, plugin),
        // Transform / spatial
        S::TransformFx => {}
        S::Crop => k_crop(buf, params, plugin),
        S::CornerPin => k_corner_pin(buf, params, plugin),
        S::Card3d => k_card_3d(buf, params, plugin),
        S::Mirror => k_mirror(buf, params, plugin),
        S::Repeat => k_repeat(buf, params, plugin),
        S::Offset => k_offset(buf, params, plugin),
        S::Reframe => k_reframe(buf, params, plugin),
        // Cleanup
        S::Denoise => k_denoise(buf, params, plugin),
        S::Deband => k_deband(buf, params, plugin),
        S::Degrain => k_degrain(buf, params, plugin),
        S::Deblur => k_deblur(buf, params, plugin),
        S::DustRemoval => k_dust_removal(buf, params, plugin),
        S::ScratchRemoval => k_scratch_removal(buf, params, plugin),
        S::DeadPixel => k_dead_pixel(buf, params, plugin),
        // Generators
        S::Solid => k_solid(buf, params, plugin, colors),
        S::FractalGen => k_fractal_gen(
            buf,
            params,
            plugin,
            colors,
            ctx,
        ),
        S::GridGen => k_grid(buf, params, plugin, colors),
        S::Shapes => k_shapes(buf, params, plugin, colors),
        S::PlasmaGen => k_plasma(buf, params, plugin, colors, ctx),
        S::Particles => k_particles(buf, params, plugin, colors, ctx),
    }
}

fn k_box_blur(buf: &mut FloatBuf, params: &[f32], plugin: StockPlugin) {
    box_blur(buf, stock_p(plugin, params, 0));
}

fn k_directional(buf: &mut FloatBuf, params: &[f32], plugin: StockPlugin) {
    dir_blur(buf, stock_p(plugin, params, 0), stock_p(plugin, params, 1));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_for(frame: i64) -> RasterFx {
        RasterFx {
            time_s: frame as f32 / 30.0,
            frame,
            res_w: 64.0,
            res_h: 64.0,
            duration_s: 5.0,
            playing: false,
        }
    }

    fn test_buf() -> FloatBuf {
        let mut buf = FloatBuf::clear(48, 48);
        for y in 0..48 {
            for x in 0..48 {
                let v = (x as f32 / 48.0 * 0.8 + 0.1 + (y as f32 / 48.0) * 0.1).min(1.0);
                buf.px[(y * 48 + x) as usize] = Px { r: v, g: v * 0.7, b: v * 0.4, a: 1.0 };
            }
        }
        // Transparent corner exercises alpha guards.
        for y in 0..8 {
            for x in 0..8 {
                buf.px[(y * 48 + x) as usize] = Px::clear();
            }
        }
        buf
    }

    fn assert_finite(buf: &FloatBuf, plugin: StockPlugin) {
        for p in &buf.px {
            for v in [p.r, p.g, p.b, p.a] {
                assert!(v.is_finite(), "{plugin:?} produced non-finite {v}");
                assert!((0.0..=1.0).contains(&v), "{plugin:?} out of range {v}");
            }
        }
    }

    #[test]
    fn every_stock_kernel_runs_clean() {
        use project::{stock_color_slots, stock_default_color};
        for plugin in StockPlugin::all() {
            let desc = plugin.descriptor();
            let colors: Vec<Color> = stock_color_slots(*plugin)
                .iter()
                .map(|s| stock_default_color(*plugin, s))
                .collect();
            let sets = [
                desc.params.iter().map(|p| p.default).collect::<Vec<_>>(),
                desc.params.iter().map(|p| p.min).collect::<Vec<_>>(),
                desc.params.iter().map(|p| p.max).collect::<Vec<_>>(),
            ];
            for params in sets {
                for frame in [0, 7] {
                    let mut buf = test_buf();
                    apply_stock(&mut buf, *plugin, &params, &colors, &ctx_for(frame));
                    assert_finite(&buf, *plugin);
                }
            }
        }
    }

    #[test]
    fn box_blur_converges_and_corner_pin_identity() {
        let mut buf = test_buf();
        let before = buf.px[(24 * 48 + 24) as usize];
        box_blur(&mut buf, 0.0);
        assert_eq!(buf.px[(24 * 48 + 24) as usize].r, before.r);
        // Identity corner pin is a pass-through.
        let mut buf = test_buf();
        let snapshot: Vec<f32> = buf.px.iter().map(|p| p.r).collect();
        k_corner_pin(&mut buf, &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0], StockPlugin::CornerPin);
        let after: Vec<f32> = buf.px.iter().map(|p| p.r).collect();
        assert_eq!(buf.px.len(), snapshot.len());
        for (a, b) in snapshot.iter().zip(after.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn corner_pin_squeeze_maps_content() {
        // Squeeze the right edge to center: output x=25% must sample
        // source x=50% (backward-mapped homography).
        let mut buf = test_buf();
        let src_mid = buf.px[(24 * 48 + 24) as usize].r;
        k_corner_pin(
            &mut buf,
            &[0.0, 0.0, 0.5, 0.0, 0.5, 1.0, 0.0, 1.0],
            StockPlugin::CornerPin,
        );
        let got = buf.px[(24 * 48 + 12) as usize].r;
        assert!(
            (got - src_mid).abs() < 0.05,
            "squeezed output samples the middle: got {got}, want {src_mid}"
        );
    }

    #[test]
    fn homography_rejects_degenerate_quads() {
        assert!(homography([(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]).is_none());
        let h = homography([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]).unwrap();
        assert!((h[0] - 1.0).abs() < 1e-5 && (h[8] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn long_shadow_extends_and_saber_adds_light() {
        use project::{stock_default_color, stock_color_slots};
        // Opaque 8x8 block top-left of a 32x32 buffer; shadow at 0° (+x).
        let mut buf = FloatBuf::clear(32, 32);
        for y in 4..12 {
            for x in 4..12 {
                buf.px[(y * 32 + x) as usize] = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            }
        }
        let cols: Vec<Color> = stock_color_slots(StockPlugin::LongShadow)
            .iter()
            .map(|s| stock_default_color(StockPlugin::LongShadow, s))
            .collect();
        // angle=0, distance=16, steps=16, fade=0, opacity=100, soft=0,
        // expand=0, mode=behind, strength=100, stride=1.
        apply_stock(&mut buf, StockPlugin::LongShadow, &[0.0, 16.0, 16.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 1.0], &cols, &ctx_for(0));
        // Source block intact, shadow present to its right, far corner clean.
        assert!(buf.px[(8 * 32 + 8) as usize].a > 0.9);
        assert!(buf.px[(8 * 32 + 20) as usize].a > 0.05, "shadow must extend +x");
        assert!(buf.px[(31 * 32 + 31) as usize].a < 0.01);
        // Shadow-only mode clears the source but keeps the tail.
        let mut only = FloatBuf::clear(32, 32);
        for y in 4..12 {
            for x in 4..12 {
                only.px[(y * 32 + x) as usize] = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            }
        }
        apply_stock(&mut only, StockPlugin::LongShadow, &[0.0, 16.0, 16.0, 0.0, 100.0, 0.0, 0.0, 2.0, 100.0, 1.0], &cols, &ctx_for(0));
        // Shadow-only: no source compositing — the extrusion volume covers
        // the source footprint too, while far pixels stay clean.
        assert!(only.px[(8 * 32 + 8) as usize].a > 0.05);
        assert!(only.px[(8 * 32 + 20) as usize].a > 0.05);
        assert!(only.px[(31 * 32 + 31) as usize].a < 0.01);
        // Saber on a mid-gray card must not dim it (additive-style glow).
        let mut card = FloatBuf::clear(24, 24);
        for p in card.px.iter_mut() {
            *p = Px { r: 0.4, g: 0.4, b: 0.4, a: 1.0 };
        }
        let scol: Vec<Color> = stock_color_slots(StockPlugin::Saber)
            .iter()
            .map(|s| stock_default_color(StockPlugin::Saber, s))
            .collect();
        let before: f32 = card.px.iter().map(|p| p.r).sum();
        apply_stock(&mut card, StockPlugin::Saber, &[6.0, 42.0, 35.0, 45.0, 25.0, 2.0, 30.0, 15.0, 30.0, 0.0, 100.0, 0.0], &scol, &ctx_for(3));
        let after: f32 = card.px.iter().map(|p| p.r).sum();
        assert!(after >= before, "saber must not dim the card");
        assert_finite(&card, StockPlugin::Saber);
    }
}
