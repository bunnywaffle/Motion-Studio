//! Auto-trace: channel field → vector mask contours.
//!
//! Mirrors After Effects' **Layer > Auto-trace**: the alpha / red / green /
//! blue / luminance channel of a layer is thresholded into a binary field,
//! connected foreground islands are traced into closed Bézier contours, and
//! background holes fully enclosed by an island are traced as hole contours
//! (applied as Subtract masks, exactly how AE handles compound paths such
//! as *i* and *e* in Create Masks From Text).
//!
//! Option mapping to the AE Auto-trace dialog:
//! - `threshold_pct` → **Threshold** (% of channel value required).
//! - `tolerance_px` → **Tolerance** (max px deviation of the traced path,
//!   implemented as Douglas-Peucker epsilon).
//! - `min_area_px` → **Minimum Area** (islands smaller than this, in px²,
//!   are dropped).
//! - `corner_roundness` → **Corner Roundness** 0..100 (Chaikin smoothing
//!   passes: 0, 1, or 2).
//! - `invert` → **Invert** (trace the inverse channel).
//! - `blur` → **Blur** (one 3×3 pre-blur to kill speckle before tracing).
//! - `apply_to_new_layer` / `range` drive the caller (new solid vs same
//!   layer; current frame vs work-area keyframes), matching the dialog.

use crate::path::{Path, PathPoint};
use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

/// Which channel of the layer feeds the tracer (field values in 0..1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceChannel {
    /// Layer alpha.
    #[default]
    Alpha,
    Luminance,
    Red,
    Green,
    Blue,
}

impl TraceChannel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Alpha => "Alpha",
            Self::Luminance => "Luminance",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
        }
    }

    pub const fn cycle(self) -> Self {
        match self {
            Self::Alpha => Self::Luminance,
            Self::Luminance => Self::Red,
            Self::Red => Self::Green,
            Self::Green => Self::Blue,
            Self::Blue => Self::Alpha,
        }
    }
}

/// Auto-trace frame range: keyframes at the playhead only, or one set of
/// path keyframes per frame across the composition work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceRange {
    #[default]
    CurrentFrame,
    WorkArea,
}

impl TraceRange {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CurrentFrame => "Current Frame",
            Self::WorkArea => "Work Area",
        }
    }
}

/// Full Auto-trace option set (mirrors the AE dialog).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTraceOptions {
    pub channel: TraceChannel,
    /// 0..100: channel % required for a pixel to count as inside.
    pub threshold_pct: f32,
    /// Max px deviation of the traced contour (simplification epsilon).
    pub tolerance_px: f32,
    /// Islands smaller than this area (px²) are dropped.
    pub min_area_px: f32,
    /// 0..100: curve smoothing at vertices.
    pub corner_roundness: f32,
    pub invert: bool,
    pub blur: bool,
    /// Put the masks on a new solid instead of the traced layer.
    pub apply_to_new_layer: bool,
    pub range: TraceRange,
}

impl Default for AutoTraceOptions {
    fn default() -> Self {
        Self {
            channel: TraceChannel::Alpha,
            threshold_pct: 50.0,
            tolerance_px: 1.0,
            min_area_px: 8.0,
            corner_roundness: 0.0,
            invert: false,
            blur: false,
            apply_to_new_layer: false,
            range: TraceRange::CurrentFrame,
        }
    }
}

/// One traced contour in field-px coords (caller adds the content origin).
#[derive(Debug, Clone)]
pub struct TracedContour {
    /// Closed polygon (pixel coords).
    pub points: Vec<Vec2>,
    /// True for holes (render as Subtract masks).
    pub is_hole: bool,
    /// Island pixel area (holes report their own area).
    pub area: f32,
}

impl TracedContour {
    /// Closed Bézier path for this contour (corner nodes; smoothing is
    /// already baked into the polygon by corner roundness).
    pub fn to_path(&self) -> Path {
        Path {
            points: self.points.iter().map(|&p| PathPoint::corner(p)).collect(),
            closed: true,
        }
    }
}

/// Hard cap on contour vertices (pathological speckle guard; the
/// simplifier tightens epsilon automatically past this).
const MAX_CONTOUR_POINTS: usize = 1500;

/// Trace a 0..1 channel field (`w*h` values) into closed contours.
pub fn trace_field(w: u32, h: u32, field: &[f32], opts: &AutoTraceOptions) -> Vec<TracedContour> {
    if w == 0 || h == 0 || field.len() < (w as usize) * (h as usize) {
        return Vec::new();
    }
    let (w, h) = (w as usize, h as usize);
    let field = field.to_vec();
    let field = if opts.blur { box_blur(&field, w, h) } else { field };
    let thr = (opts.threshold_pct.clamp(0.0, 100.0) / 100.0).clamp(0.0, 1.0);
    let inside: Vec<bool> = field
        .iter()
        .map(|&v| {
            let v = if opts.invert { 1.0 - v } else { v };
            v >= thr
        })
        .collect();

    // Foreground islands.
    let mut contours = Vec::new();
    for comp in connected_components(&inside, w, h, true) {
        if comp.area < opts.min_area_px.max(0.0) {
            continue;
        }
        let raw = trace_component(&inside, w, h, &comp);
        let poly = finalize_polygon(&raw, opts);
        if poly.len() < 3 {
            continue;
        }
        contours.push(TracedContour { points: poly, is_hole: false, area: comp.area });
    }
    // Background holes (components not touching the image border).
    let fg_polys: Vec<Vec<Vec2>> = contours.iter().map(|c| c.points.clone()).collect();
    for comp in connected_components(&inside, w, h, false) {
        if comp.touches_border {
            continue;
        }
        if comp.area < opts.min_area_px.max(0.0) {
            continue;
        }
        // Holes belong to whoever contains them (AE: compound-path holes
        // subtract from their own island).
        let seed = Vec2::new(comp.seed_x as f32, comp.seed_y as f32);
        if !fg_polys.iter().any(|p| point_in_poly(seed, p)) {
            continue;
        }
        // Trace the hole region itself (its boundary == the hole contour).
        let inverted: Vec<bool> = inside
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let x = i % w;
                let y = i / w;
                x >= comp.min_x && x <= comp.max_x && y >= comp.min_y && y <= comp.max_y && !inside[i]
            })
            .collect();
        let hole_comp = Component {
            pixels: comp.pixels.clone(),
            area: comp.area,
            min_x: comp.min_x,
            min_y: comp.min_y,
            max_x: comp.max_x,
            max_y: comp.max_y,
            seed_x: comp.seed_x,
            seed_y: comp.seed_y,
            touches_border: false,
        };
        let raw = trace_component(&inverted, w, h, &hole_comp);
        let poly = finalize_polygon(&raw, opts);
        if poly.len() < 3 {
            continue;
        }
        contours.push(TracedContour { points: poly, is_hole: true, area: comp.area });
    }
    // Islands first (Add), holes after (Subtract) — combination order.
    contours.sort_by_key(|a| a.is_hole as u8);
    contours
}

/// One 3×3 box-blur pass (speckle killer for the Blur option).
fn box_blur(field: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0f32;
            let mut n = 0.0f32;
            for oy in -1i32..=1 {
                for ox in -1i32..=1 {
                    let (sx, sy) = (x as i32 + ox, y as i32 + oy);
                    if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                        sum += field[sy as usize * w + sx as usize];
                        n += 1.0;
                    }
                }
            }
            out[y * w + x] = sum / n.max(1.0);
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Component {
    pixels: Vec<u32>,
    area: f32,
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
    seed_x: usize,
    seed_y: usize,
    touches_border: bool,
}

/// 4-connected components of `target` cells in `inside`.
fn connected_components(inside: &[bool], w: usize, h: usize, target: bool) -> Vec<Component> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    for start in 0..w * h {
        if seen[start] || inside[start] != target {
            continue;
        }
        let mut pixels = Vec::new();
        let mut stack = vec![start];
        seen[start] = true;
        let (mut min_x, mut min_y) = (w, h);
        let (mut max_x, mut max_y) = (0usize, 0usize);
        let mut touches_border = false;
        while let Some(i) = stack.pop() {
            pixels.push(i as u32);
            let (x, y) = (i % w, i / w);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                touches_border = true;
            }
            if x > 0 && !seen[i - 1] && inside[i - 1] == target {
                seen[i - 1] = true;
                stack.push(i - 1);
            }
            if x + 1 < w && !seen[i + 1] && inside[i + 1] == target {
                seen[i + 1] = true;
                stack.push(i + 1);
            }
            if y > 0 && !seen[i - w] && inside[i - w] == target {
                seen[i - w] = true;
                stack.push(i - w);
            }
            if y + 1 < h && !seen[i + w] && inside[i + w] == target {
                seen[i + w] = true;
                stack.push(i + w);
            }
        }
        let area = pixels.len() as f32;
        let (seed_x, seed_y) = ((pixels[0] as usize) % w, (pixels[0] as usize) / w);
        out.push(Component { pixels, area, min_x, min_y, max_x, max_y, seed_x, seed_y, touches_border });
    }
    // Big islands first (stable mask order).
    out.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Moore-neighbor boundary trace with Jacob's stopping criterion.
/// Returns boundary pixel centers in order.
fn trace_component(inside: &[bool], w: usize, h: usize, comp: &Component) -> Vec<Vec2> {
    // 8-neighbors clockwise from North.
    const NB: [(i32, i32); 8] = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    let at = |x: i32, y: i32| -> bool {
        if x < comp.min_x as i32 - 1
            || y < comp.min_y as i32 - 1
            || x > comp.max_x as i32 + 1
            || y > comp.max_y as i32 + 1
            || x < 0
            || y < 0
            || x >= w as i32
            || y >= h as i32
        {
            return false;
        }
        inside[(y as usize) * w + (x as usize)]
    };
    // Start: topmost, then leftmost pixel of the component bbox scan.
    let mut start: Option<(i32, i32)> = None;
    'outer: for y in comp.min_y..=comp.max_y {
        for x in comp.min_x..=comp.max_x {
            if at(x as i32, y as i32) {
                start = Some((x as i32, y as i32));
                break 'outer;
            }
        }
    }
    let start = match start {
        Some(s) => s,
        None => return Vec::new(),
    };
    // West of the row-leftmost start is background (or out of bounds).
    let init_back = (start.0 - 1, start.1);
    let dir_of = |from: (i32, i32), to: (i32, i32)| -> usize {
        let d = (to.0 - from.0, to.1 - from.1);
        NB.iter().position(|&n| n == d).unwrap_or(0)
    };
    let mut boundary = vec![Vec2::new(start.0 as f32, start.1 as f32)];
    let (mut back, mut cur) = (init_back, start);
    let max_steps = comp.pixels.len() * 4 + 64;
    for _ in 0..max_steps {
        let k = dir_of(cur, back);
        let mut next: Option<(i32, i32)> = None;
        // Clockwise scan starting after the backtrack pixel.
        for o in 1..=8 {
            let (dx, dy) = NB[(k + o) % 8];
            let cand = (cur.0 + dx, cur.1 + dy);
            if at(cand.0, cand.1) {
                next = Some(cand);
                break;
            }
        }
        let next = match next {
            Some(n) => n,
            None => break,
        };
        back = cur;
        cur = next;
        if cur == start && back == init_back {
            break;
        }
        // Avoid runaway on 1-px-wide diagonals: stop revisiting start from
        // any direction twice.
        if cur == start {
            break;
        }
        boundary.push(Vec2::new(cur.0 as f32, cur.1 as f32));
    }
    boundary
}

/// Simplify + smooth a raw contour into its final polygon.
fn finalize_polygon(raw: &[Vec2], opts: &AutoTraceOptions) -> Vec<Vec2> {
    if raw.len() < 3 {
        return Vec::new();
    }
    let mut eps = opts.tolerance_px.max(0.05);
    let mut poly = douglas_peucker(raw, eps);
    // Pathological speckle guard: tighten until the vertex budget fits.
    while poly.len() > MAX_CONTOUR_POINTS && eps < 32.0 {
        eps *= 2.0;
        poly = douglas_peucker(raw, eps);
    }
    if poly.len() < 3 {
        return Vec::new();
    }
    let passes = (opts.corner_roundness.clamp(0.0, 100.0) / 50.0).round() as usize;
    let mut smooth = poly;
    for _ in 0..passes.min(2) {
        smooth = chaikin_closed(&smooth);
    }
    if smooth.len() < 3 {
        return Vec::new();
    }
    smooth
}

/// Iterative Douglas-Peucker on a closed loop (keeps endpoints stable).
fn douglas_peucker(pts: &[Vec2], eps: f32) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    // Anchor at the bbox-extreme point so the loop split is deterministic.
    let mut anchor = 0;
    for (i, p) in pts.iter().enumerate() {
        if (p.x, p.y) < (pts[anchor].x, pts[anchor].y) {
            anchor = i;
        }
    }
    let n = pts.len();
    // Open the loop at the anchor and simplify as an open polyline.
    let ordered: Vec<Vec2> = (0..n).map(|i| pts[(anchor + i) % n]).collect();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (ordered[a], ordered[b]);
        let mut best = 0.0f32;
        let mut idx = a;
        for (i, p) in ordered.iter().enumerate().take(b).skip(a + 1) {
            let d = point_seg_dist(*p, pa, pb);
            if d > best {
                best = d;
                idx = i;
            }
        }
        if best > eps {
            keep[idx] = true;
            stack.push((a, idx));
            stack.push((idx, b));
        }
    }
    ordered.into_iter().enumerate().filter(|(i, _)| keep[*i]).map(|(_, p)| p).collect()
}

fn point_seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 < 1e-12 {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len2).clamp(0.0, 1.0);
    let (cx, cy) = (a.x + abx * t, a.y + aby * t);
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

/// One Chaikin corner-cutting pass on a closed loop.
fn chaikin_closed(pts: &[Vec2]) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len() * 2);
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        out.push(Vec2::new(p.x * 0.75 + q.x * 0.25, p.y * 0.75 + q.y * 0.25));
        out.push(Vec2::new(p.x * 0.25 + q.x * 0.75, p.y * 0.25 + q.y * 0.75));
    }
    out
}

fn point_in_poly(p: Vec2, poly: &[Vec2]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_rect(w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<f32> {
        let mut f = vec![0.0f32; w * h];
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                f[y * w + x] = 1.0;
            }
        }
        f
    }

    #[test]
    fn solid_rect_traces_one_island() {
        let f = field_rect(64, 64, 10, 12, 40, 44);
        let cs = trace_field(64, 64, &f, &AutoTraceOptions::default());
        assert_eq!(cs.len(), 1, "{cs:?}");
        assert!(!cs[0].is_hole);
        // Tolerance 1px over a 30x32 rect: small vertex count.
        assert!(cs[0].points.len() <= 12, "{}", cs[0].points.len());
        let (mut mnx, mut mxx) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut mny, mut mxy) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in &cs[0].points {
            mnx = mnx.min(p.x);
            mxx = mxx.max(p.x);
            mny = mny.min(p.y);
            mxy = mxy.max(p.y);
        }
        assert!((mnx - 10.0).abs() <= 2.0 && (mxx - 39.0).abs() <= 2.0, "{mnx} {mxx}");
        assert!((mny - 12.0).abs() <= 2.0 && (mxy - 43.0).abs() <= 2.0, "{mny} {mxy}");
        assert!(cs[0].to_path().closed);
    }

    #[test]
    fn hole_becomes_subtract_contour() {
        // Donut: outer rect with an interior background hole.
        let mut f = field_rect(64, 64, 8, 8, 56, 56);
        for y in 24..40 {
            for x in 24..40 {
                f[y * 64 + x] = 0.0;
            }
        }
        let cs = trace_field(64, 64, &f, &AutoTraceOptions::default());
        assert_eq!(cs.len(), 2, "{cs:?}");
        assert!(!cs[0].is_hole && cs[1].is_hole, "islands first, holes after");
    }

    #[test]
    fn min_area_filters_speckle() {
        let mut f = field_rect(64, 64, 8, 8, 56, 56);
        f[2 * 64 + 2] = 1.0; // lone speckle pixel
        let opts = AutoTraceOptions {
            min_area_px: 4.0,
            ..Default::default()
        };
        let cs = trace_field(64, 64, &f, &opts);
        assert_eq!(cs.len(), 1, "{cs:?}");
    }

    #[test]
    fn invert_and_threshold_behave() {
        // Full-white field inverted at 50% -> nothing.
        let f = vec![1.0f32; 32 * 32];
        let mut opts = AutoTraceOptions {
            invert: true,
            ..Default::default()
        };
        assert!(trace_field(32, 32, &f, &opts).is_empty());
        // 40% gray passes a 30% threshold, fails a 50% one.
        let g = vec![0.4f32; 32 * 32];
        opts.invert = false;
        opts.threshold_pct = 30.0;
        assert_eq!(trace_field(32, 32, &g, &opts).len(), 1);
        opts.threshold_pct = 50.0;
        assert!(trace_field(32, 32, &g, &opts).is_empty());
    }

    #[test]
    fn roundness_smooths_without_collapse() {
        let f = field_rect(64, 64, 10, 10, 50, 50);
        let opts = AutoTraceOptions {
            corner_roundness: 100.0,
            ..Default::default()
        };
        let cs = trace_field(64, 64, &f, &opts);
        assert_eq!(cs.len(), 1);
        assert!(cs[0].points.len() >= 8, "chaikin doubles corners");
        assert!(cs[0].to_path().closed);
    }
}
