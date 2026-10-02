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
    if poly.len() < 3 || w == 0 || h == 0 {
        return;
    }
    struct Edge {
        y_top: f32,
        y_bot: f32,
        x_top: f32,
        dx: f32,
    }
    let mut edges: Vec<Edge> = Vec::with_capacity(poly.len());
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        min_y = min_y.min(a.y);
        max_y = max_y.max(a.y);
        let dy = b.y - a.y;
        if dy.abs() < 1e-6 {
            continue; // horizontal — skip
        }
        let (top, bot) = if a.y < b.y { (a, b) } else { (b, a) };
        edges.push(Edge {
            y_top: top.y,
            y_bot: bot.y,
            x_top: top.x,
            dx: (bot.x - top.x) / (bot.y - top.y),
        });
    }
    if edges.is_empty() {
        return;
    }
    let y_lo = (min_y - 0.5).floor().max(0.0) as i32;
    let y_hi = ((max_y + 0.5).ceil() as i32).min(h as i32);
    let mut x_intercepts: Vec<f32> = Vec::new();
    for y in y_lo..y_hi {
        let y_scan = y as f32 + 0.5;
        x_intercepts.clear();
        for e in &edges {
            // Half-open interval [y_top, y_bot) at scanline center y_scan:
            // ensures exact parity and prevents stray edge extrapolations
            // that cause horizontal tearing streaks on curved paths/ellipses.
            if e.y_top <= y_scan && y_scan < e.y_bot {
                let x = e.x_top + (y_scan - e.y_top) * e.dx;
                x_intercepts.push(x);
            }
        }
        if x_intercepts.len() < 2 {
            continue;
        }
        x_intercepts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let row_start = (y as u32 * w) as usize;
        let row = &mut coverage[row_start..row_start + w as usize];
        let mut k = 0;
        while k + 1 < x_intercepts.len() {
            let xa = x_intercepts[k].min(x_intercepts[k + 1]).max(0.0);
            let xb = x_intercepts[k].max(x_intercepts[k + 1]).min(w as f32);
            if xb > xa {
                let start_x = xa.floor() as usize;
                let end_x = (xb.ceil() as usize).min(w as usize);
                for (offset, cur) in row[start_x..end_x].iter_mut().enumerate() {
                    let x = start_x + offset;
                    let px_left = x as f32;
                    let px_right = px_left + 1.0;
                    let cov = (xb.min(px_right) - xa.max(px_left)).clamp(0.0, 1.0);
                    *cur = (*cur + cov).min(1.0);
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
    mask_coverage_mapped(w, h, path, &transform.local_matrix)
}

/// Coverage with an explicit path→buffer map (fast-preview path composes
/// the work→output map here so coverage evaluates at output res).
pub(crate) fn mask_coverage_mapped(
    w: u32,
    h: u32,
    path: &Path,
    map: &compositor::AffineTransform2D,
) -> Vec<f32> {
    let mut cov = vec![0.0f32; (w.max(1) * h.max(1)) as usize];
    if w == 0 || h == 0 || path.is_empty() {
        return cov;
    }
    let m = map;
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
/// Shared with the text outline (morphological dilation of glyph alpha).
pub(crate) fn box_extremum(cov: &[f32], w: u32, h: u32, radius: f32, is_max: bool) -> Vec<f32> {
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
/// Compute the combined mask coverage alpha map (0.0..=1.0) for a given buffer size.
/// Returns None if no enabled, closed masks constrain the area.
pub fn evaluate_mask_coverage(w: u32, h: u32, masks: &[EvaluatedMask]) -> Option<Vec<f32>> {
    evaluate_mask_coverage_mapped(
        w,
        h,
        masks,
        &compositor::AffineTransform2D::IDENTITY,
        1.0,
    )
}

/// Coverage with an explicit layer-local→evaluation-space map and radius
/// scale: the fast-preview path evaluates at output res (`space_map` folds
/// the work→output map, radii shrink by the output/work ratio) and upscales
/// the smooth result, instead of filtering full-res buffers per frame.
pub fn evaluate_mask_coverage_mapped(
    w: u32,
    h: u32,
    masks: &[EvaluatedMask],
    space_map: &compositor::AffineTransform2D,
    radius_scale: f32,
) -> Option<Vec<f32>> {
    if w == 0 || h == 0 {
        return None;
    }
    let radius_scale = radius_scale.clamp(0.0, 1.0);
    let mut acc = vec![1.0f32; (w * h) as usize];
    let mut has = false;
    for mask in masks {
        if !mask.enabled || mask.mode == MaskMode::None {
            continue;
        }
        // Fully transparent masks contribute their mode neutral, which is
        // the identity for every combine mode — skip them outright.
        if mask.opacity < 0.5 {
            continue;
        }
        // An unclosed mask path or mask with fewer than 3 vertices cannot enclose any 2D area.
        // In After Effects, open/incomplete masks do NOT clip the layer content.
        if !mask.path.closed || mask.path.points.len() < 3 {
            continue;
        }
        let map = *space_map * mask.transform.local_matrix;
        // 1. Coverage from the transformed path.
        let mut cov = mask_coverage_mapped(w, h, &mask.path, &map);
        // 2. Expansion (positive dilates, negative erodes).
        let expansion = mask.expansion * radius_scale;
        if expansion > 0.05 {
            cov = box_extremum(&cov, w, h, expansion, true);
        } else if expansion < -0.05 {
            cov = box_extremum(&cov, w, h, -expansion, false);
        }
        // 3. Feather.
        cov = feather_coverage(&cov, w, h, mask.feather * radius_scale);
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
        let mut next_has = has;
        for (a, c) in acc.iter_mut().zip(cov.iter()) {
            let ce = n + (c - n) * o;
            let (v, h) = project::mask::combine_mask_coverage(mask.mode, *a, ce, has);
            *a = v;
            next_has = h;
        }
        has = next_has;
    }
    if has {
        Some(acc)
    } else {
        None
    }
}

/// Apply the full mask stack to `buf` alpha (premultiplied scale).
/// No-op when no mask constrains (zero enabled non-None masks).
pub fn apply_masks(buf: &mut FloatBuf, masks: &[EvaluatedMask]) {
    if let Some(acc) = evaluate_mask_coverage(buf.w, buf.h, masks) {
        for (p, &m) in buf.px.iter_mut().zip(acc.iter()) {
            p.r *= m;
            p.g *= m;
            p.b *= m;
            p.a *= m;
        }
    }
}

/// Apply output-res coverage (fast-preview path) to a work buffer:
/// bilinear-upsample `cov` (`cw` × `ch`) into buffer space, then
/// premultiplied-scale like [`apply_masks`].
pub(crate) fn apply_scaled_coverage(
    buf: &mut FloatBuf,
    cov: &[f32],
    cw: u32,
    ch: u32,
) {
    if cw == 0 || ch == 0 || cov.len() < (cw.max(1) * ch.max(1)) as usize {
        return;
    }
    let sample = |x: f32, y: f32| -> f32 {
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fx = (x - x0 as f32).clamp(0.0, 1.0);
        let fy = (y - y0 as f32).clamp(0.0, 1.0);
        let at = |ix: i32, iy: i32| -> f32 {
            let ix = ix.clamp(0, cw as i32 - 1) as usize;
            let iy = iy.clamp(0, ch as i32 - 1) as usize;
            cov[iy * cw as usize + ix]
        };
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        a * (1.0 - fx) * (1.0 - fy) + b * fx * (1.0 - fy) + c * (1.0 - fx) * fy + d * fx * fy
    };
    for y in 0..buf.h {
        for x in 0..buf.w {
            let sx = (x as f32 + 0.5) / buf.w.max(1) as f32 * cw as f32 - 0.5;
            let sy = (y as f32 + 0.5) / buf.h.max(1) as f32 * ch as f32 - 0.5;
            let m = sample(sx, sy).clamp(0.0, 1.0);
            let p = &mut buf.px[(y * buf.w + x) as usize];
            p.r *= m;
            p.g *= m;
            p.b *= m;
            p.a *= m;
        }
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
    fn ellipse_coverage_has_no_horizontal_streak_artifacts() {
        let cov = mask_coverage(64, 64, &Path::ellipse(32.0, 32.0, 16.0, 12.0), &EvaluatedTransform::IDENTITY);
        assert_eq!(cov.len(), 64 * 64);
        // Center covered
        assert!(cov[32 * 64 + 32] > 0.99);
        // All outer pixels MUST have zero coverage (no horizontal streaks across the buffer)
        for y in 0..64 {
            assert_eq!(cov[y * 64], 0.0, "streak at ({y}, 0)");
            assert_eq!(cov[y * 64 + 1], 0.0, "streak at ({y}, 1)");
            assert_eq!(cov[y * 64 + 62], 0.0, "streak at ({y}, 62)");
            assert_eq!(cov[y * 64 + 63], 0.0, "streak at ({y}, 63)");
        }
        for y in 0..10 {
            for x in 0..64 {
                assert_eq!(cov[y * 64 + x], 0.0, "streak at row {y}, col {x}");
                assert_eq!(cov[(63 - y) * 64 + x], 0.0, "streak at row {}, col {x}", 63 - y);
            }
        }
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
    fn scaled_coverage_matches_full_within_tolerance() {
        // Feathered rect: full-res coverage vs half-res (scaled radii)
        // upscaled back — smooth coverage must agree closely.
        use compositor::AffineTransform2D;
        use project::Vec2;
        let m = EvaluatedMask {
            id: "m".to_string(),
            name: "M".to_string(),
            enabled: true,
            path: Path::rectangle(20.0, 20.0, 160.0, 160.0),
            mode: MaskMode::Add,
            opacity: 100.0,
            feather: 6.0,
            expansion: 3.0,
            invert: false,
            transform: EvaluatedTransform::IDENTITY,
        };
        let full = evaluate_mask_coverage(200, 200, std::slice::from_ref(&m)).expect("full");
        let half_map = AffineTransform2D::from_scale(Vec2::new(0.5, 0.5));
        let small =
            evaluate_mask_coverage_mapped(100, 100, std::slice::from_ref(&m), &half_map, 0.5)
                .expect("scaled");
        let mut up = FloatBuf::clear(200, 200);
        for p in up.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        apply_scaled_coverage(&mut up, &small, 100, 100);
        let mut worst = 0.0f32;
        let mut flat_bad = 0usize;
        for (i, p) in up.px.iter().enumerate() {
            let d = (p.a - full[i]).abs();
            worst = worst.max(d);
            // Pixels safely away from the rect edge band must match tightly;
            // resampling only perturbs the ~1px edge band.
            let (x, y) = (i % 200, i / 200);
            let near_edge = !(30..=170).contains(&x) || !(30..=170).contains(&y);
            if !near_edge && d > 0.02 {
                flat_bad += 1;
            }
        }
        assert_eq!(flat_bad, 0, "scaled coverage must match off-edge");
        assert!(worst < 0.25, "scaled coverage edge drift {worst}");
    }

    #[test]
    fn feather_keeps_deep_interior_opaque() {
        // 80x80 box in a 100x100 buffer, feather 8: the center (40px from
        // any edge, ~10 sigma out) must stay fully covered.
        let cov = mask_coverage(
            100,
            100,
            &Path::rectangle(10.0, 10.0, 80.0, 80.0),
            &EvaluatedTransform::IDENTITY,
        );
        assert!(cov[50 * 100 + 50] > 0.99, "raw coverage center");
        let soft = feather_coverage(&cov, 100, 100, 8.0);
        assert!(soft[50 * 100 + 50] > 0.99, "feathered center {}", soft[50 * 100 + 50]);
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
