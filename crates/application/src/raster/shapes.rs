use super::buffer::FloatBuf;
use super::pixel::{gradient_axis, sample_fill_gradient, GradientAxis, Px};
use project::FillGradient;

// Shape SDF fills (local px, straight alpha)
// ---------------------------------------------------------------------------

pub(crate) fn fill_rect(buf: &mut FloatBuf, w: f32, h: f32, cr: f32, col: Px) {
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

pub(crate) fn fill_rect_gradient(
    buf: &mut FloatBuf,
    w: f32,
    h: f32,
    cr: f32,
    gradient: &FillGradient,
) {
    let axis = gradient_axis(w, h, gradient.angle);
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
                let mut p = Px::from_color(sample_fill_gradient(gradient, fx, fy, axis));
                // Cheap 1px AA on the outer edge.
                let edge = (w - fx).min(fx).min(h - fy).min(fy);
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

pub(crate) fn fill_ellipse(buf: &mut FloatBuf, rx: f32, ry: f32, col: Px) {
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

pub(crate) fn fill_ellipse_gradient(
    buf: &mut FloatBuf,
    rx: f32,
    ry: f32,
    gradient: &FillGradient,
) {
    let (cx, cy) = (buf.w as f32 / 2.0, buf.h as f32 / 2.0);
    let axis = gradient_axis(buf.w as f32, buf.h as f32, gradient.angle);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let fx = (x as f32 + 0.5 - cx) / rx.max(0.5);
            let fy = (y as f32 + 0.5 - cy) / ry.max(0.5);
            let d = fx * fx + fy * fy;
            if d <= 1.0 {
                let mut p = Px::from_color(sample_fill_gradient(
                    gradient,
                    x as f32 + 0.5,
                    y as f32 + 0.5,
                    axis,
                ));
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

/// Fill a path through the shared path model using even-odd polygon fill.
/// `origin` is the content-buffer frame origin in path-local coords.
pub(crate) fn fill_path(
    buf: &mut FloatBuf,
    path_data: &str,
    col: Px,
    origin: project::Vec2,
) {
    let path = project::Path::from_svg(path_data);
    let mut pts = path.flatten(0.5);
    if pts.len() < 3 {
        return;
    }
    for p in pts.iter_mut() {
        *p -= origin;
    }
    let mut cov = vec![0.0f32; (buf.w * buf.h) as usize];
    super::mask::fill_even_odd(&mut cov, buf.w, buf.h, &pts);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let a = cov[(y * buf.w + x) as usize];
            if a > 0.0 {
                let mut p = col;
                p.scale(a);
                let dst = buf.get(x as i32, y as i32);
                let mut out = dst;
                out.over(p);
                buf.put(x as i32, y as i32, out);
            }
        }
    }
}

/// Gradient variant of [`fill_path`]: coverage from the path, color from
/// the linear gradient over the content frame (`frame_size`).
pub(crate) fn fill_path_gradient(
    buf: &mut FloatBuf,
    path_data: &str,
    gradient: &FillGradient,
    origin: project::Vec2,
    frame_size: (f32, f32),
) {
    let path = project::Path::from_svg(path_data);
    let mut pts = path.flatten(0.5);
    if pts.len() < 3 {
        return;
    }
    for p in pts.iter_mut() {
        *p -= origin;
    }
    let axis = gradient_axis(frame_size.0, frame_size.1, gradient.angle);
    let mut cov = vec![0.0f32; (buf.w * buf.h) as usize];
    super::mask::fill_even_odd(&mut cov, buf.w, buf.h, &pts);
    for y in 0..buf.h {
        for x in 0..buf.w {
            let a = cov[(y * buf.w + x) as usize];
            if a > 0.0 {
                let c = sample_fill_gradient(gradient, x as f32 + 0.5, y as f32 + 0.5, axis);
                let p = Px::from_color_scaled(c, a);
                let dst = buf.get(x as i32, y as i32);
                let mut out = dst;
                out.over(p);
                buf.put(x as i32, y as i32, out);
            }
        }
    }
}

/// Stroke line cap (open-path ends).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LineCap {
    Butt,
    #[default]
    Round,
    Square,
}

/// Stroke line join (polyline vertices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LineJoin {
    Miter,
    #[default]
    Round,
    Bevel,
}

/// Resolved stroke style for [`stroke_path`]/[`stroke_path_gradient`].
/// Caps clip or extend the nib walk at open ends; bevel joins clip the
/// round bulge at vertices (one half-plane each — exact, since the plane
/// contains the joint's outer-edge endpoints); miter joins stamp the spike
/// triangle (miter-limit 5 widths, then bevel). Dashes gate whole nib
/// stamps along arclength; dash islands keep round ends.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StrokeStyle {
    pub cap: LineCap,
    pub join: LineJoin,
    pub dash: f32,
    pub gap: f32,
    pub offset: f32,
}

impl StrokeStyle {
    /// Parse shape `line_cap`/`line_join` strings (unknown → round) with
    /// dash params in px (0 = solid).
    pub(crate) fn from_shape(cap: &str, join: &str, dash: f32, gap: f32, offset: f32) -> Self {
        let cap = match cap.to_lowercase().as_str() {
            "butt" => LineCap::Butt,
            "square" => LineCap::Square,
            _ => LineCap::Round,
        };
        let join = match join.to_lowercase().as_str() {
            "miter" => LineJoin::Miter,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Round,
        };
        Self { cap, join, dash: dash.max(0.0), gap: gap.max(0.0), offset }
    }

    fn dashed(self) -> bool {
        self.dash > 0.0 && self.gap > 0.0
    }
}

/// Per-pixel paint for nib stamps (solid color or linear gradient).
enum Paint<'a> {
    Solid(Px),
    Gradient(&'a FillGradient, GradientAxis),
}

fn paint_px(paint: &Paint, bx: i32, by: i32) -> Px {
    match paint {
        Paint::Solid(col) => *col,
        Paint::Gradient(gradient, axis) => Px::from_color(sample_fill_gradient(
            gradient,
            bx as f32 + 0.5,
            by as f32 + 0.5,
            *axis,
        )),
    }
}

/// Half-plane clip: keep pixels whose center satisfies
/// `(center - point) · normal >= 0`.
type Clip = (project::Vec2, project::Vec2);

/// Nib stamp (disc of radius `r`) with half-plane clips. Addressing matches
/// the legacy `dot()` exactly so solid round strokes rasterize
/// pixel-identically with or without the style machinery.
fn stamp(
    buf: &mut FloatBuf,
    x: f32,
    y: f32,
    r: f32,
    paint: &Paint,
    clips: &[Clip],
) {
    let r2 = r * r;
    for oy in (-r.ceil() as i32)..=(r.ceil() as i32) {
        for ox in (-r.ceil() as i32)..=(r.ceil() as i32) {
            let dx = ox as f32 + 0.5;
            let dy = oy as f32 + 0.5;
            if dx * dx + dy * dy > r2 {
                continue;
            }
            let (bx, by) = (x as i32 + ox, y as i32 + oy);
            let cx = bx as f32 + 0.5;
            let cy = by as f32 + 0.5;
            let mut kept = true;
            for (point, normal) in clips {
                if (cx - point.x) * normal.x + (cy - point.y) * normal.y < 0.0 {
                    kept = false;
                    break;
                }
            }
            if !kept {
                continue;
            }
            let col = paint_px(paint, bx, by);
            let dst = buf.get(bx, by);
            let mut out = dst;
            out.over(col);
            buf.put(bx, by, out);
        }
    }
}

/// Solid triangle stamp (miter spikes). Winding-independent edge test.
fn stamp_triangle(buf: &mut FloatBuf, a: project::Vec2, b: project::Vec2, c: project::Vec2, paint: &Paint) {
    let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if area.abs() < 1e-6 {
        return;
    }
    let (min_x, max_x) = (a.x.min(b.x).min(c.x).floor() as i32, a.x.max(b.x).max(c.x).ceil() as i32);
    let (min_y, max_y) = (a.y.min(b.y).min(c.y).floor() as i32, a.y.max(b.y).max(c.y).ceil() as i32);
    let edge = |p: project::Vec2, q: project::Vec2, x: f32, y: f32| {
        (x - p.x) * (q.y - p.y) - (y - p.y) * (q.x - p.x)
    };
    for by in min_y..=max_y {
        for bx in min_x..=max_x {
            let (cx, cy) = (bx as f32 + 0.5, by as f32 + 0.5);
            let (e0, e1, e2) = (edge(a, b, cx, cy), edge(b, c, cx, cy), edge(c, a, cx, cy));
            let inside = if area > 0.0 {
                e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0
            } else {
                e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0
            };
            if !inside {
                continue;
            }
            let col = paint_px(paint, bx, by);
            let dst = buf.get(bx, by);
            let mut out = dst;
            out.over(col);
            buf.put(bx, by, out);
        }
    }
}

/// Outer-side unit normals + miter data for the joint at `v` between
/// incoming dir `u` and outgoing dir `v_dir` (both normalized). Returns
/// `(outer_n_in, outer_n_out, miter_point_or_bevel_plane)`: miter point
/// when within the limit, else the bevel clip plane as
/// `(plane_point, outward_normal)`.
fn joint_geometry(
    v: project::Vec2,
    u: project::Vec2,
    v_dir: project::Vec2,
    nib: f32,
) -> (project::Vec2, project::Vec2, Result<project::Vec2, Clip>) {
    // Left normals; the outer side is opposite the turn direction.
    let (l1, l2) = (
        project::Vec2::new(-u.y, u.x),
        project::Vec2::new(-v_dir.y, v_dir.x),
    );
    let cross = u.x * v_dir.y - u.y * v_dir.x;
    let (n1, n2) = if cross > 0.0 { (-l1, -l2) } else { (l1, l2) };
    let denom = 1.0 + n1.dot(n2);
    if denom < 1e-4 {
        // Doubling back: miter is infinite → bevel through V. The plane
        // normal points back toward V (keep side); the bulge beyond the
        // chord is cut.
        let m = (n1 + n2).normalize();
        return (n1, n2, Err((v + m * nib, -m)));
    }
    let miter = v + (n1 + n2) * (nib / denom);
    // Miter limit 5 widths (10 nibs), SVG-style, then bevel.
    if (miter.x - v.x).hypot(miter.y - v.y) > nib * 10.0 {
        let m = (n1 + n2).normalize();
        (n1, n2, Err((v + (n1 + n2) * (nib * 0.5), -m)))
    } else {
        (n1, n2, Ok(miter))
    }
}

/// Stroke a path through the shared path model (curves flatten to the
/// polyline, then a styled nib walks it). Closed paths join up.
/// `origin` is the content-buffer frame origin in path-local coords
/// (see `Path::frame`): the path is drawn shifted by `-origin` so buffers
/// spanning arbitrary local coords are never clipped.
pub(crate) fn stroke_path(
    buf: &mut FloatBuf,
    path_data: &str,
    nib: f32,
    col: Px,
    origin: project::Vec2,
    style: StrokeStyle,
) {
    let path = project::Path::from_svg(path_data);
    let mut pts = path.flatten(0.5);
    if pts.is_empty() {
        return;
    }
    for p in pts.iter_mut() {
        *p -= origin;
    }
    let paint = Paint::Solid(col);
    stroke_polyline(buf, &pts, path.closed, nib, &paint, style);
}

/// Gradient variant of [`stroke_path`]: each nib stamp samples the linear
/// gradient over the content frame (`frame_size`).
pub(crate) fn stroke_path_gradient(
    buf: &mut FloatBuf,
    path_data: &str,
    nib: f32,
    gradient: &FillGradient,
    origin: project::Vec2,
    frame_size: (f32, f32),
    style: StrokeStyle,
) {
    let path = project::Path::from_svg(path_data);
    let mut pts = path.flatten(0.5);
    if pts.is_empty() {
        return;
    }
    for p in pts.iter_mut() {
        *p -= origin;
    }
    let axis = gradient_axis(frame_size.0, frame_size.1, gradient.angle);
    let paint = Paint::Gradient(gradient, axis);
    stroke_polyline(buf, &pts, path.closed, nib, &paint, style);
}

/// Stroke a polyline with full style support. `pts` are buffer-space
/// points (closed paths repeat the first point at the end, as produced
/// below). 1px walk steps match the legacy stroker's coverage.
fn stroke_polyline(
    buf: &mut FloatBuf,
    pts: &[project::Vec2],
    closed: bool,
    nib: f32,
    paint: &Paint,
    style: StrokeStyle,
) {
    let seg_count = if closed { pts.len() } else { pts.len().saturating_sub(1) };
    if seg_count == 0 {
        if let Some(p) = pts.first() {
            stamp(buf, p.x, p.y, nib, paint, &[]);
        }
        return;
    }
    let at = |i: usize| pts[i % pts.len()];
    let seg_len = |i: usize| at(i).distance_to(at(i + 1));
    // Arclength at each segment start.
    let mut starts = Vec::with_capacity(seg_count + 1);
    let mut acc = 0.0;
    for i in 0..seg_count {
        starts.push(acc);
        acc += seg_len(i);
    }
    let period = style.dash + style.gap;
    // Join vertex indices (closed: every vertex; open: interior only).
    let join_at: Vec<usize> = if closed {
        (0..pts.len()).collect()
    } else {
        (1..pts.len().saturating_sub(1)).collect()
    };
    // Resolve bevel planes + miter triangles up front.
    // bevels: vertex idx → clip plane; miters: triangles to stamp after.
    let mut bevels: Vec<(usize, Clip)> = Vec::new();
    let mut miters: Vec<(project::Vec2, project::Vec2, project::Vec2)> = Vec::new();
    if style.join != LineJoin::Round {
        for &vi in &join_at {
            let n = pts.len();
            let prev = at(vi + n - 1);
            let v = at(vi);
            let next = at(vi + 1);
            let (lu, lv) = (v - prev, next - v);
            if lu.length() < 1e-6 || lv.length() < 1e-6 {
                continue;
            }
            let (u, vv) = (lu.normalize(), lv.normalize());
            let (n1, n2, res) = joint_geometry(v, u, vv, nib);
            match (&res, style.join) {
                (Ok(miter), LineJoin::Miter) => {
                    miters.push((v + n1 * nib, *miter, v + n2 * nib));
                }
                _ => {
                    let plane = match &res {
                        Ok(_) => {
                            let m = (n1 + n2).normalize();
                            (v + (n1 + n2) * (nib * 0.5), -m)
                        }
                        Err(c) => *c,
                    };
                    bevels.push((vi, plane));
                }
            }
        }
    }
    let bevel_at = |vi: usize| bevels.iter().find(|(i, _)| *i == vi).map(|(_, c)| *c);
    for si in 0..seg_count {
        let (a, b) = (at(si), at(si + 1));
        let len = seg_len(si);
        if len < 1e-6 {
            continue;
        }
        let dir = (b - a) * (1.0 / len);
        // Square caps overhang one nib at open ends.
        let (ext0, ext1) = match style.cap {
            LineCap::Square if !closed => (
                if si == 0 { nib } else { 0.0 },
                if si == seg_count - 1 { nib } else { 0.0 },
            ),
            _ => (0.0, 0.0),
        };
        let j0 = (-ext0).floor() as i32;
        let j1 = (len + ext1).ceil() as i32;
        for j in j0..=j1 {
            let d = j as f32;
            let p = a + dir * d;
            let s = starts[si] + d;
            if style.dashed()
                && (s + style.offset).rem_euclid(period) >= style.dash
            {
                continue;
            }
            let mut clips: [Clip; 4] = [(project::Vec2::ZERO, project::Vec2::ZERO); 4];
            let mut nc = 0;
            // Cap planes at open ends (butt clips at the path end,
            // square clips at the overhang end; round needs none).
            // Both ends apply on single-segment paths.
            if !closed {
                let is_first = si == 0;
                let is_last = si == seg_count - 1;
                match style.cap {
                    LineCap::Butt => {
                        if is_first {
                            clips[nc] = (a, dir);
                            nc += 1;
                        }
                        if is_last {
                            clips[nc] = (b, -dir);
                            nc += 1;
                        }
                    }
                    LineCap::Square => {
                        if is_first {
                            clips[nc] = (a - dir * nib, dir);
                            nc += 1;
                        }
                        if is_last {
                            clips[nc] = (b + dir * nib, -dir);
                            nc += 1;
                        }
                    }
                    LineCap::Round => {}
                }
            }
            // Bevel planes at incident join vertices.
            if style.join != LineJoin::Round {
                let start_vi = si % pts.len();
                let end_vi = (si + 1) % pts.len();
                let start_is_join = closed || si > 0;
                let end_is_join = closed || si + 1 < seg_count;
                if start_is_join {
                    if let Some(c) = bevel_at(start_vi) {
                        clips[nc] = c;
                        nc += 1;
                    }
                }
                if end_is_join {
                    if let Some(c) = bevel_at(end_vi) {
                        clips[nc] = c;
                        nc += 1;
                    }
                }
            }
            stamp(buf, p.x, p.y, nib, paint, &clips[..nc]);
        }
    }
    for (a, m, b) in miters {
        stamp_triangle(buf, a, m, b, paint);
    }
}

// ---------------------------------------------------------------------------
