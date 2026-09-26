//! Mask pipeline rasterizer: Path → Coverage → Feather / Expansion →
//! Combination → Layer Alpha.
//!
//! Each enabled mask fills its (mask-transformed) path coverage, shapes it
//! with expansion (max/min filter) and feather (gaussian), blends toward
//! the mode neutral by opacity, then combines in mask order with
//! [`project::mask::combine_mask_coverage`] — the exact raster-space
//! equivalent of Add / Subtract / Intersect / Difference booleans. The
//! final accumulation scales layer alpha (premultiplied store).

use super::buffer::{FloatBuf, blur_buffer};
use super::pixel::Px;
use compositor::{EvaluatedMask, EvaluatedTransform};
use project::{MaskMode, Path};

/// Even-odd scanline fill of a closed polygon into `coverage` (writes 1.0;
/// caller clears first). Coordinates are buffer px.
pub(crate) fn fill_even_odd(coverage: &mut [f32], w: u32, h: u32, poly: &[project::Vec2]) {
    if poly.len() < 3 {
        return;
    }
    // Edge table: (y_min, y_max, x_at_ymin, dx_per_y).
    struct Edge {
        y_min: i32,
        y_max: i32,
        x: f32,
        dx: f32,
    }
    let mut edges: Vec<Edge> = Vec::new();
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y - b.y).abs() < 1e-9 {
            continue;
        }
        let (top, bot) = if a.y < b.y { (a, b) } else { (b, a) };
        edges.push(Edge {
            y_min: top.y.floor() as i32,
            y_max: bot.y.ceil() as i32,
            x: top.x,
            dx: (bot.x - top.x) / (bot.y - top.y).max(1e-9),
        });
    }
    if edges.is_empty() {
        return;
    }
    let hi = h as i32;
    let y_lo = edges.iter().map(|e| e.y_min).min().unwrap().max(0);
    let y_hi = edges.iter().map(|e| e.y_max).max().unwrap().min(hi);
    let mut active: Vec<usize> = Vec::new();
    for y in y_lo..y_hi {
        active.clear();
        for (i, e) in edges.iter().enumerate() {
            if y >= e.y_min && y < e.y_max {
                active.push(i);
            }
        }
        if active.len() < 2 {
            continue;
        }
        active.sort_by(|&a, &b| {
            let xa = edges[a].x + (y as f32 + 0.5 - edges[a].y_min as f32) * edges[a].dx;
            let xb = edges[b].x + (y as f32 + 0.5 - edges[b].y_min as f32) * edges[b].dx;
            xa.partial_cmp(&xb).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut k = 0;
        while k + 1 < active.len() {
            let xa = edges[active[k]].x
                + (y as f32 + 0.5 - edges[active[k]].y_min as f32) * edges[active[k]].dx;
            let xb = edges[active[k + 1]].x
                + (y as f32 + 0.5 - edges[active[k + 1]].y_min as f32) * edges[active[k + 1]].dx;
            let x0 = xa.ceil().max(0.0) as i32;
            let x1 = (xb.floor() as i32).min(w as i32 - 1);
            // Even-odd pairing: fill between pairs (0-1, 2-3, ...).
            if x1 >= x0 {
                let row = &mut coverage[(y as u32 * w) as usize..((y as u32 + 1) * w) as usize];
                for x in x0..=x1 {
                    row[x as usize] = 1.0;
                }
            }
            k += 2;
        }
    }
}

/// Coverage of one evaluated mask in buffer px (0..1).
pub fn mask_coverage(
    w: u32,
    h: u32,
    path: &Path,
    transform: &EvaluatedTransform,
) -> Vec<f32> {
    let mut cov = vec![0.0f32; (w.max(1) * h.max(1)) as usize];
    if w == 0 || h == 0 || path.is_empty() {
        return cov;
    }
    let m = &transform.local_matrix;
    let flat = path.flatten(0.5);
    if flat.len() < 3 {
        return cov;
    }
    let poly: Vec<project::Vec2> = flat
        .into_iter()
        .map(|p| project::Vec2::new(m.a * p.x + m.c * p.y + m.tx, m.b * p.x + m.d * p.y + m.ty))
        .collect();
    fill_even_odd(&mut cov, w, h, &poly);
    cov
}

/// Separable box extremum filter (max for expansion, min for contraction).
fn box_extremum(cov: &[f32], w: u32, h: u32, radius: f32, is_max: bool) -> Vec<f32> {
    let r = radius.clamp(0.0, 128.0).floor() as i32;
    if r < 1 || w == 0 || h == 0 {
        return cov.to_vec();
    }
    let (wu, hu) = (w as usize, h as usize);
    let init = if is_max { 0.0f32 } else { 1.0f32 };
    // Horizontal pass (centered windows; radii here are small so the
    // direct neighborhood scan stays interactive).
    let mut tmp = vec![init; cov.len()];
    for y in 0..hu {
        for x in 0..wu {
            let mut best = init;
            for dx in -r..=r {
                let v = cov[y * wu + (x as i32 + dx).clamp(0, wu as i32 - 1) as usize];
                best = if is_max { best.max(v) } else { best.min(v) };
            }
            tmp[y * wu + x] = best;
        }
    }
    // Vertical pass.
    let mut out = vec![init; cov.len()];
    for x in 0..wu {
        for y in 0..hu {
            let mut best = init;
            for dy in -r..=r {
                let v = tmp[((y as i32 + dy).clamp(0, hu as i32 - 1) as usize) * wu + x];
                best = if is_max { best.max(v) } else { best.min(v) };
            }
            out[y * wu + x] = best;
        }
    }
    out
}

/// Feather coverage with a gaussian blur (reuses the tested kernel).
fn feather_coverage(cov: &[f32], w: u32, h: u32, radius: f32) -> Vec<f32> {
    if radius < 0.25 || w == 0 || h == 0 {
        return cov.to_vec();
    }
    let mut buf = FloatBuf::clear(w, h);
    for (d, &c) in buf.px.iter_mut().zip(cov.iter()) {
        *d = Px { r: c, g: c, b: c, a: c };
    }
    blur_buffer(&mut buf, radius);
    buf.px.iter().map(|p| p.a.clamp(0.0, 1.0)).collect()
}

/// Apply the full mask stack to `buf` alpha (premultiplied scale).
/// No-op when no mask constrains (zero enabled non-None masks).
pub fn apply_masks(buf: &mut FloatBuf, masks: &[EvaluatedMask]) {
    if buf.w == 0 || buf.h == 0 {
        return;
    }
    let mut acc = vec![1.0f32; (buf.w * buf.h) as usize];
    let mut has = false;
    for mask in masks {
        if !mask.enabled || mask.mode == MaskMode::None {
            continue;
        }
        // An unclosed mask path or mask with fewer than 3 vertices cannot enclose any 2D area.
        // In After Effects, open/incomplete masks do NOT clip the layer content.
        if !mask.path.closed || mask.path.points.len() < 3 {
            continue;
        }
        // 1. Coverage from the transformed path.
        let mut cov = mask_coverage(buf.w, buf.h, &mask.path, &mask.transform);
        // 2. Expansion (positive dilates, negative erodes).
        if mask.expansion > 0.05 {
            cov = box_extremum(&cov, buf.w, buf.h, mask.expansion, true);
        } else if mask.expansion < -0.05 {
            cov = box_extremum(&cov, buf.w, buf.h, -mask.expansion, false);
        }
        // 3. Feather.
        cov = feather_coverage(&cov, buf.w, buf.h, mask.feather);
        // 4. Invert.
        if mask.invert {
            for c in cov.iter_mut() {
                *c = 1.0 - *c;
            }
        }
        // 5. Opacity toward the mode neutral.
        let n = mask.mode.neutral();
        let o = (mask.opacity / 100.0).clamp(0.0, 1.0);
        // 6. Combine in mask order.
        for (a, c) in acc.iter_mut().zip(cov.iter()) {
            let ce = n + (c - n) * o;
            let (v, h) = project::mask::combine_mask_coverage(mask.mode, *a, ce, has);
            *a = v;
            has = h;
        }
    }
    if !has {
        return;
    }
    for (p, &m) in buf.px.iter_mut().zip(acc.iter()) {
        p.r *= m;
        p.g *= m;
        p.b *= m;
        p.a *= m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::EvaluatedTransform;
    use project::Path;

    fn test_mask(path: Path) -> EvaluatedMask {
        EvaluatedMask {
            id: "m".to_string(),
            name: "M".to_string(),
            enabled: true,
            path,
            mode: MaskMode::Add,
            opacity: 100.0,
            feather: 0.0,
            expansion: 0.0,
            invert: false,
            transform: EvaluatedTransform::IDENTITY,
        }
    }

    #[test]
    fn rect_coverage_fills_interior() {
        let cov = mask_coverage(32, 32, &Path::rectangle(8.0, 8.0, 16.0, 16.0), &EvaluatedTransform::IDENTITY);
        assert_eq!(cov.len(), 32 * 32);
        // Center covered, corners empty.
        assert!(cov[16 * 32 + 16] > 0.99);
        assert_eq!(cov[0], 0.0);
        assert_eq!(cov[31 * 32 + 31], 0.0);
        let filled: usize = cov.iter().filter(|&&c| c > 0.5).count();
        assert!((filled as i32 - 256).abs() < 40, "{filled}");
    }

    #[test]
    fn add_mask_cuts_alpha_outside() {
        let mut buf = FloatBuf::clear(32, 32);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        apply_masks(&mut buf, &[test_mask(Path::rectangle(8.0, 8.0, 16.0, 16.0))]);
        assert!(buf.px[16 * 32 + 16].a > 0.99);
        assert_eq!(buf.px[0].a, 0.0);
    }

    #[test]
    fn subtract_and_invert() {
        let mut buf = FloatBuf::clear(32, 32);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let mut m = test_mask(Path::rectangle(8.0, 8.0, 16.0, 16.0));
        m.mode = MaskMode::Subtract;
        apply_masks(&mut buf, &[m]);
        assert_eq!(buf.px[16 * 32 + 16].a, 0.0);
        assert!(buf.px[0].a > 0.99);
    }

    #[test]
    fn feather_softens_and_expansion_grows() {
        let cov = mask_coverage(32, 32, &Path::rectangle(8.0, 8.0, 16.0, 16.0), &EvaluatedTransform::IDENTITY);
        let soft = feather_coverage(&cov, 32, 32, 4.0);
        // Edge pixel (8,16) was 1.0 hard; feather pulls it fractional.
        assert!(soft[16 * 32 + 8] < 1.0 && soft[16 * 32 + 8] > 0.0);
        // Deep interior stays ~1.
        assert!(soft[16 * 32 + 16] > 0.9);
        let grown = box_extremum(&cov, 32, 32, 4.0, true);
        assert!(grown[16 * 32 + 4] > 0.5, "expansion reaches x=4");
        assert_eq!(grown[16 * 32 + 2], 0.0, "expansion stops before x=2");
        let shrunk = box_extremum(&cov, 32, 32, 4.0, false);
        assert!(shrunk[16 * 32 + 11] < 0.5, "erosion clears x=11");
        assert!(shrunk[16 * 32 + 16] > 0.5, "erosion keeps deep interior");
    }
}
