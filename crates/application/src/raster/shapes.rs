use super::buffer::FloatBuf;
use super::pixel::Px;

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

/// Stroke a path through the shared path model (curves flatten to the
/// polyline, then a round-ish nib walks it). Closed paths join up.
/// `origin` is the content-buffer frame origin in path-local coords
/// (see `Path::frame`): the path is drawn shifted by `-origin` so buffers
/// spanning arbitrary local coords are never clipped.
pub(crate) fn stroke_path(
    buf: &mut FloatBuf,
    path_data: &str,
    nib: f32,
    col: Px,
    origin: project::Vec2,
) {
    let path = project::Path::from_svg(path_data);
    let mut pts = path.flatten(0.5);
    if pts.is_empty() {
        return;
    }
    for p in pts.iter_mut() {
        *p -= origin;
    }
    if path.closed {
        pts.push(pts[0]);
    }
    if pts.len() < 2 {
        // Single point: draw a dot.
        dot(buf, pts[0].x, pts[0].y, nib, col);
        return;
    }
    for w in pts.windows(2) {
        stroke_segment(buf, (w[0].x, w[0].y), (w[1].x, w[1].y), nib, col);
    }
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
