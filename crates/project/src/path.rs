//! Shared vector path system.
//!
//! One Bézier path model used by masks, pen/shape paths, motion paths, and
//! (through sampling) text-on-path. Design notes:
//! - Points carry **relative** tangent offsets (`in_tan`/`out_tan` added to
//!   `pos`), so translating a path never touches its handles.
//! - [`PathPointKind`] drives handle coupling in editors (corner / smooth /
//!   symmetric / auto), mirroring After Effects.
//! - [`Path`] is [`Interpolate`], so mask/shape paths keyframe and morph
//!   with the existing keyframe engine (same point count + closed flag,
//!   otherwise the track holds/steps).
//! - Boolean ops flatten to polygons and run Greiner–Hormann; mask
//!   *modes* instead combine at coverage level in the rasterizer (exact,
//!   no vector surgery needed).

use crate::keyframe::Interpolate;
use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

/// Handle coupling mode of a path node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathPointKind {
    /// Independent in/out handles (sharp corner possible).
    #[default]
    Corner,
    /// Handles stay collinear (180° apart), lengths independent.
    Smooth,
    /// Handles stay collinear with mirrored lengths.
    Symmetric,
    /// Handles auto-fit from neighbours (Catmull-Rom style) on edit.
    Auto,
}

/// One Bézier node: anchor plus relative tangent offsets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathPoint {
    pub pos: Vec2,
    #[serde(default)]
    pub in_tan: Vec2,
    #[serde(default)]
    pub out_tan: Vec2,
    #[serde(default)]
    pub kind: PathPointKind,
}

impl PathPoint {
    /// Sharp corner node with no handles.
    pub const fn corner(pos: Vec2) -> Self {
        Self { pos, in_tan: Vec2::ZERO, out_tan: Vec2::ZERO, kind: PathPointKind::Corner }
    }

    /// Smooth node with explicit relative handles.
    pub const fn smooth(pos: Vec2, in_tan: Vec2, out_tan: Vec2) -> Self {
        Self { pos, in_tan, out_tan, kind: PathPointKind::Smooth }
    }

    /// Absolute incoming handle tip.
    pub fn in_abs(&self) -> Vec2 {
        self.pos + self.in_tan
    }

    /// Absolute outgoing handle tip.
    pub fn out_abs(&self) -> Vec2 {
        self.pos + self.out_tan
    }

    /// Set the incoming handle from an absolute tip, enforcing the point kind.
    pub fn set_in_abs(&mut self, tip: Vec2) {
        self.in_tan = tip - self.pos;
        self.enforce_kind(true);
    }

    /// Set the outgoing handle from an absolute tip, enforcing the point kind.
    pub fn set_out_abs(&mut self, tip: Vec2) {
        self.out_tan = tip - self.pos;
        self.enforce_kind(false);
    }

    /// Move the anchor, dragging both handles along (offsets preserved).
    pub fn move_to(&mut self, pos: Vec2) {
        self.pos = pos;
    }

    /// Re-couple the opposite handle after one side changed.
    /// `changed_in` is true when the incoming handle was edited.
    pub fn enforce_kind(&mut self, changed_in: bool) {
        match self.kind {
            PathPointKind::Corner => {}
            PathPointKind::Smooth => {
                // Keep collinear: mirror the edited direction, keep the
                // other side's length.
                let (dir, other_len) = if changed_in {
                    (self.in_tan, self.out_tan.length())
                } else {
                    (self.out_tan, self.in_tan.length())
                };
                if dir.length() > 1e-6 && other_len > 1e-6 {
                    let n = dir.normalize();
                    if changed_in {
                        self.out_tan = Vec2::new(-n.x, -n.y) * other_len;
                    } else {
                        self.in_tan = Vec2::new(-n.x, -n.y) * other_len;
                    }
                }
            }
            PathPointKind::Symmetric => {
                if changed_in {
                    self.out_tan = Vec2::new(-self.in_tan.x, -self.in_tan.y);
                } else {
                    self.in_tan = Vec2::new(-self.out_tan.x, -self.out_tan.y);
                }
            }
            PathPointKind::Auto => {
                // Auto is resolved against neighbours by `Path::autofit`;
                // a manual edit degrades to smooth coupling.
                self.kind = PathPointKind::Smooth;
                self.enforce_kind(changed_in);
            }
        }
    }

    /// Convert to another kind, adjusting handles.
    pub fn convert_to(&mut self, kind: PathPointKind) {
        self.kind = kind;
        match kind {
            PathPointKind::Corner => {
                self.in_tan = Vec2::ZERO;
                self.out_tan = Vec2::ZERO;
            }
            PathPointKind::Symmetric => {
                // Mirror the longer handle.
                if self.out_tan.length() >= self.in_tan.length() {
                    self.in_tan = Vec2::new(-self.out_tan.x, -self.out_tan.y);
                } else {
                    self.out_tan = Vec2::new(-self.in_tan.x, -self.in_tan.y);
                }
            }
            PathPointKind::Smooth | PathPointKind::Auto => {}
        }
    }
}

impl Interpolate for PathPoint {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            pos: self.pos.lerp(other.pos, t),
            in_tan: self.in_tan.lerp(other.in_tan, t),
            out_tan: self.out_tan.lerp(other.out_tan, t),
            kind: if t < 1.0 { self.kind } else { other.kind },
        }
    }
}

/// A cubic Bézier spline: nodes plus open/closed flag.
///
/// Segment `i` runs from `points[i]` to `points[(i+1) % n]` with control
/// points `p_i + out_i` and `p_{i+1} + in_{i+1}`. Open paths use segments
/// `0..n-1`; closed paths add the wrap segment.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Path {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<PathPoint>,
    #[serde(default)]
    pub closed: bool,
}

impl Path {
    /// Empty open path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Closed rectangle path (clockwise, corner nodes).
    pub fn rectangle(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            points: vec![
                PathPoint::corner(Vec2::new(x, y)),
                PathPoint::corner(Vec2::new(x + w, y)),
                PathPoint::corner(Vec2::new(x + w, y + h)),
                PathPoint::corner(Vec2::new(x, y + h)),
            ],
            closed: true,
        }
    }

    /// Closed ellipse approximation (4 smooth nodes, kappa handles).
    pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Self {
        const KAPPA: f32 = 0.5522847498;
        let (kx, ky) = (KAPPA * rx, KAPPA * ry);
        Self {
            points: vec![
                PathPoint::smooth(Vec2::new(cx + rx, cy), Vec2::new(0.0, -ky), Vec2::new(0.0, ky)),
                PathPoint::smooth(Vec2::new(cx, cy + ry), Vec2::new(kx, 0.0), Vec2::new(-kx, 0.0)),
                PathPoint::smooth(Vec2::new(cx - rx, cy), Vec2::new(0.0, ky), Vec2::new(0.0, -ky)),
                PathPoint::smooth(Vec2::new(cx, cy - ry), Vec2::new(-kx, 0.0), Vec2::new(kx, 0.0)),
            ],
            closed: true,
        }
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// True when there are no nodes.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Number of cubic segments.
    pub fn segment_count(&self) -> usize {
        let n = self.points.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }

    /// Segment endpoints `(p0, p1, p2, p3)` in absolute coords.
    pub fn segment(&self, i: usize) -> Option<(Vec2, Vec2, Vec2, Vec2)> {
        let n = self.points.len();
        if n < 2 || i >= self.segment_count() {
            return None;
        }
        let a = &self.points[i];
        let b = &self.points[(i + 1) % n];
        Some((a.pos, a.out_abs(), b.in_abs(), b.pos))
    }

    /// Append a corner node.
    pub fn line_to(&mut self, pos: Vec2) {
        self.points.push(PathPoint::corner(pos));
    }

    /// Append a node with explicit handles.
    pub fn curve_to(&mut self, pos: Vec2, in_tan: Vec2, out_tan: Vec2, kind: PathPointKind) {
        self.points.push(PathPoint { pos, in_tan, out_tan, kind });
    }

    /// Close the path (join last node to first).
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Open the path.
    pub fn open(&mut self) {
        self.closed = false;
    }

    /// Reverse point order (keeps geometry, flips direction). Handle
    /// offsets are swapped so tangents still describe the same curve.
    pub fn reversed(&self) -> Self {
        let mut points: Vec<PathPoint> = self
            .points
            .iter()
            .rev()
            .map(|p| PathPoint {
                pos: p.pos,
                in_tan: p.out_tan,
                out_tan: p.in_tan,
                kind: p.kind,
            })
            .collect();
        // Keep the same start anchor for predictable morphing.
        if self.closed && !points.is_empty() {
            let first = points.remove(points.len() - 1);
            points.insert(0, first);
        }
        Self { points, closed: self.closed }
    }

    /// Append another open path's nodes to this one.
    pub fn join(&mut self, other: &Path) {
        self.points.extend(other.points.iter().copied());
    }

    /// Insert a node subdividing segment `seg` at factor `t` (de Casteljau).
    /// Returns the new node index, or None for bad segments.
    pub fn split_segment(&mut self, seg: usize, t: f32) -> Option<usize> {
        let n = self.points.len();
        if n < 2 || seg >= self.segment_count() {
            return None;
        }
        let t = t.clamp(0.0, 1.0);
        let (p0, p1, p2, p3) = self.segment(seg)?;
        let lerp = |a: Vec2, b: Vec2| a.lerp(b, t);
        let q0 = lerp(p0, p1);
        let q1 = lerp(p1, p2);
        let q2 = lerp(p2, p3);
        let r0 = lerp(q0, q1);
        let r1 = lerp(q1, q2);
        let s = lerp(r0, r1);
        let j = (seg + 1) % n.max(1);
        // Left segment keeps p0, right keeps p3; node handles are absolute
        // offsets rewritten relative to the anchors.
        self.points[seg].out_tan = q0 - p0;
        let mut node = PathPoint::corner(s);
        node.in_tan = r0 - s;
        node.out_tan = r1 - s;
        // Preserve smooth coupling where both neighbours were smooth.
        let left_smooth = self.points[seg].kind != PathPointKind::Corner;
        let right_smooth = self.points[j].kind != PathPointKind::Corner;
        node.kind = if left_smooth && right_smooth {
            PathPointKind::Smooth
        } else {
            PathPointKind::Corner
        };
        self.points[j].in_tan = q2 - p3;
        let insert_at = if seg + 1 >= n && self.closed { 0 } else { seg + 1 };
        if self.closed && seg == n - 1 {
            self.points.push(node);
            Some(n)
        } else {
            self.points.insert(insert_at, node);
            Some(insert_at)
        }
    }

    /// Remove node `i`. Keeps at least... allows emptying; returns removed.
    pub fn remove_point(&mut self, i: usize) -> Option<PathPoint> {
        if i >= self.points.len() {
            return None;
        }
        Some(self.points.remove(i))
    }

    /// Refit all `Auto` nodes from neighbours (Catmull-Rom tangents).
    pub fn autofit(&mut self) {
        let n = self.points.len();
        if n < 2 {
            return;
        }
        for i in 0..n {
            if self.points[i].kind != PathPointKind::Auto {
                continue;
            }
            let prev = if i == 0 {
                if self.closed { self.points[n - 1].pos } else { self.points[i].pos }
            } else {
                self.points[i - 1].pos
            };
            let next = if i + 1 == n {
                if self.closed { self.points[0].pos } else { self.points[i].pos }
            } else {
                self.points[i + 1].pos
            };
            let tangent = (next - prev) * (1.0 / 6.0);
            self.points[i].in_tan = Vec2::new(-tangent.x, -tangent.y);
            self.points[i].out_tan = tangent;
        }
    }

    /// Evaluate segment `seg` at factor `t`.
    pub fn eval_segment(&self, seg: usize, t: f32) -> Option<Vec2> {
        let (p0, p1, p2, p3) = self.segment(seg)?;
        let t = t.clamp(0.0, 1.0);
        let u = 1.0 - t;
        Some(Vec2::new(
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
        ))
    }

    /// Adaptive flatten into a polyline (tolerance in path units).
    pub fn flatten(&self, tolerance: f32) -> Vec<Vec2> {
        let tol = tolerance.max(0.01);
        let mut out = Vec::new();
        for s in 0..self.segment_count() {
            let (p0, p1, p2, p3) = match self.segment(s) {
                Some(v) => v,
                None => continue,
            };
            if s == 0 {
                out.push(p0);
            }
            flatten_cubic(p0, p1, p2, p3, tol, &mut out);
            out.push(p3);
        }
        out
    }

    /// Axis-aligned bounding box, or None when empty.
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        // Tight-ish bounds from control hulls (cheap, conservative).
        let mut mn = Vec2::new(f32::INFINITY, f32::INFINITY);
        let mut mx = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        if self.points.is_empty() {
            return None;
        }
        for s in 0..self.segment_count().max(1).min(self.points.len().max(1)) {
            if let Some((p0, p1, p2, p3)) = self.segment(s) {
                for p in [p0, p1, p2, p3] {
                    mn.x = mn.x.min(p.x);
                    mn.y = mn.y.min(p.y);
                    mx.x = mx.x.max(p.x);
                    mx.y = mx.y.max(p.y);
                }
            } else if self.points.len() == 1 {
                let p = self.points[0].pos;
                mn = p;
                mx = p;
            }
        }
        // Single degenerate point or segments covered above.
        if mn.x == f32::INFINITY {
            let p = self.points[0].pos;
            return Some((p, p));
        }
        Some((mn, mx))
    }

    /// Approximate total arc length (flattened).
    pub fn length(&self, tolerance: f32) -> f32 {
        let pts = self.flatten(tolerance);
        poly_length(&pts, self.closed)
    }

    /// Point at arc-length ratio `t` in `[0, 1]` (even spacing for motion
    /// paths and text-on-path). Returns None when empty.
    pub fn point_at_ratio(&self, t: f32, tolerance: f32) -> Option<Vec2> {
        let pts = self.flatten(tolerance);
        if pts.is_empty() {
            return None;
        }
        if pts.len() == 1 {
            return Some(pts[0]);
        }
        point_at_poly_ratio(&pts, self.closed, t.clamp(0.0, 1.0))
    }

    /// Tangent angle in degrees at arc-length ratio `t` (finite
    /// difference on the flattened polyline). Powers glyph orientation
    /// for text-on-path and rotation for motion-path followers.
    pub fn tangent_at_ratio(&self, t: f32, tolerance: f32) -> Option<f32> {
        let pts = self.flatten(tolerance);
        if pts.len() < 2 {
            return None;
        }
        let t = t.clamp(0.0, 1.0);
        let total = poly_length(&pts, self.closed).max(1e-9);
        let e = (0.5 / total).clamp(1e-4, 0.02);
        let a = point_at_poly_ratio(&pts, self.closed, (t - e).max(0.0))?;
        let b = point_at_poly_ratio(&pts, self.closed, (t + e).min(1.0))?;
        let d = b - a;
        if d.length_squared() < 1e-12 {
            return Some(0.0);
        }
        Some(d.y.atan2(d.x).to_degrees())
    }

    /// Evenly resample into `count` corner nodes (open keeps ends).
    pub fn resample(&self, count: usize, tolerance: f32) -> Self {
        let n = count.max(2);
        let mut points = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            if let Some(p) = self.point_at_ratio(t, tolerance) {
                points.push(PathPoint::corner(p));
            }
        }
        Self { points, closed: false }
    }

    /// Simplify with Ramer–Douglas–Peucker on the flattened polyline.
    /// Returns corner-node path preserving open/closed-ness.
    pub fn simplify(&self, tolerance: f32) -> Self {
        let pts = self.flatten(tolerance.max(0.01));
        if pts.len() < 3 {
            return self.clone();
        }
        let mut keep = vec![false; pts.len()];
        keep[0] = true;
        keep[pts.len() - 1] = true;
        rdp(&pts, 0, pts.len() - 1, tolerance.max(0.01), &mut keep);
        let points: Vec<PathPoint> = pts
            .into_iter()
            .enumerate()
            .filter(|(i, _)| keep[*i])
            .map(|(_, p)| PathPoint::corner(p))
            .collect();
        let keep_closed = self.closed && points.len() > 2;
        Self { points, closed: keep_closed }
    }

    /// Offset approximation: resample, push each sample along its normal by
    /// `distance`, rebuild as smooth nodes. Positive expands CCW shapes.
    pub fn offset(&self, distance: f32, samples_per_seg: usize) -> Self {
        if self.points.len() < 2 {
            return self.clone();
        }
        let flat = self.flatten(0.5);
        if flat.len() < 2 {
            return self.clone();
        }
        let closed_loop = self.closed;
        // Resample evenly for stable normals.
        let total = poly_length(&flat, closed_loop).max(1e-6);
        let count = ((self.segment_count() * samples_per_seg).max(8)).min(512);
        let mut pts = Vec::with_capacity(count);
        for i in 0..count {
            let t = i as f32 / count as f32;
            if let Some(p) = point_at_poly_ratio(&flat, closed_loop, t) {
                pts.push(p);
            }
        }
        let m = pts.len();
        // Outward side from winding: CCW (positive area) interiors sit left
        // of each edge, so outward is the right normal (and vice versa).
        let mut area = 0.0f32;
        for i in 0..m {
            let b = pts[(i + 1) % m];
            area += pts[i].x * b.y - b.x * pts[i].y;
        }
        let side = if area >= 0.0 { -1.0 } else { 1.0 };
        let mut out = Vec::with_capacity(m);
        for i in 0..m {
            let prev = pts[(i + m - 1) % m];
            let next = pts[(i + 1) % m];
            let mut tangent = next - prev;
            let len = tangent.length();
            if len < 1e-6 {
                out.push(PathPoint::corner(pts[i]));
                continue;
            }
            tangent = tangent / len;
            // Left normal, flipped outward per winding.
            let normal = Vec2::new(-tangent.y, tangent.x) * side;
            out.push(PathPoint::corner(pts[i] + normal * distance));
        }
        let _ = total;
        Self { points: out, closed: self.closed }
    }

    /// Affine transform `(x', y') = (a*x + c*y + tx, b*x + d*y + ty)`.
    /// Tangents transform as directions (translation excluded).
    pub fn transformed(&self, a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) -> Self {
        let map_p = |p: Vec2| Vec2::new(a * p.x + c * p.y + tx, b * p.x + d * p.y + ty);
        let map_v = |v: Vec2| Vec2::new(a * v.x + c * v.y, b * v.x + d * v.y);
        Self {
            points: self
                .points
                .iter()
                .map(|p| PathPoint {
                    pos: map_p(p.pos),
                    in_tan: map_v(p.in_tan),
                    out_tan: map_v(p.out_tan),
                    kind: p.kind,
                })
                .collect(),
            closed: self.closed,
        }
    }

    /// Translate all nodes.
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        self.transformed(1.0, 0.0, 0.0, 1.0, dx, dy)
    }

    /// Boolean op against another path (flattened polygons,
    /// Greiner–Hormann). Returns result contour paths (closed).
    pub fn boolean_op(&self, other: &Path, op: PathBooleanOp, tolerance: f32) -> Vec<Path> {
        let a = self.flatten(tolerance.max(0.1));
        let b = other.flatten(tolerance.max(0.1));
        boolean_polygons(&a, &b, op)
            .into_iter()
            .map(|poly| Path {
                points: poly.into_iter().map(PathPoint::corner).collect(),
                closed: true,
            })
            .filter(|p| p.points.len() > 2)
            .collect()
    }

    /// Parse the SVG path subset this app writes (`M/L/C/Q/Z`, absolute +
    /// relative, `H`/`V`). Unknown commands stop parsing (returns what was
    /// read). Used for `ShapeType::Path.path_data` interop.
    pub fn from_svg(data: &str) -> Self {
        let mut path = Path::new();
        let tokens = tokenize_svg(data);
        let mut i = 0;
        let mut cursor = Vec2::ZERO;
        let num = |tokens: &[SvgTok], i: &mut usize| -> Option<f32> {
            if let Some(SvgTok::Num(v)) = tokens.get(*i) {
                *i += 1;
                Some(*v)
            } else {
                None
            }
        };
        while i < tokens.len() {
            let cmd = match &tokens[i] {
                SvgTok::Cmd(c) => {
                    i += 1;
                    *c
                }
                SvgTok::Num(_) => {
                    // Implicit repeat of moveto-turned-lineto handled below.
                    break;
                }
            };
            let rel = cmd.is_ascii_lowercase();
            match cmd.to_ascii_uppercase() {
                'M' => {
                    let (ox, oy) = match (num(&tokens, &mut i), num(&tokens, &mut i)) {
                        (Some(x), Some(y)) => (x, y),
                        _ => break,
                    };
                    let (ox, oy) = if rel { (ox + cursor.x, oy + cursor.y) } else { (ox, oy) };
                    cursor = Vec2::new(ox, oy);
                    path.points.push(PathPoint::corner(cursor));
                    // Subsequent bare coordinate pairs are linetos.
                    while let (Some(nx), Some(ny)) = (
                        tokens.get(i).and_then(|t| t.as_num()),
                        tokens.get(i + 1).and_then(|t| t.as_num()),
                    ) {
                        i += 2;
                        let (px, py) = if rel { (nx + cursor.x, ny + cursor.y) } else { (nx, ny) };
                        cursor = Vec2::new(px, py);
                        path.points.push(PathPoint::corner(cursor));
                    }
                }
                'L' => {
                    while let (Some(nx), Some(ny)) = (
                        tokens.get(i).and_then(|t| t.as_num()),
                        tokens.get(i + 1).and_then(|t| t.as_num()),
                    ) {
                        i += 2;
                        let (mut px, mut py) = (nx, ny);
                        if rel {
                            px += cursor.x;
                            py += cursor.y;
                        }
                        cursor = Vec2::new(px, py);
                        path.points.push(PathPoint::corner(cursor));
                    }
                }
                'H' => {
                    while let Some(nx) = tokens.get(i).and_then(|t| t.as_num()) {
                        i += 1;
                        let mut px = nx;
                        if rel {
                            px += cursor.x;
                        }
                        cursor = Vec2::new(px, cursor.y);
                        path.points.push(PathPoint::corner(cursor));
                    }
                }
                'V' => {
                    while let Some(ny) = tokens.get(i).and_then(|t| t.as_num()) {
                        i += 1;
                        let mut py = ny;
                        if rel {
                            py += cursor.y;
                        }
                        cursor = Vec2::new(cursor.x, py);
                        path.points.push(PathPoint::corner(cursor));
                    }
                }
                'C' => {
                    while tokens.get(i).and_then(|t| t.as_num()).is_some() {
                        let vals: Vec<f32> = (0..6)
                            .map(|_| num(&tokens, &mut i).unwrap_or(0.0))
                            .collect();
                        let mut pts = [
                            Vec2::new(vals[0], vals[1]),
                            Vec2::new(vals[2], vals[3]),
                            Vec2::new(vals[4], vals[5]),
                        ];
                        if rel {
                            for p in &mut pts {
                                *p = *p + cursor;
                            }
                        }
                        // Previous node gains the outgoing handle; new node
                        // carries the incoming one.
                        if let Some(last) = path.points.last_mut() {
                            last.out_tan = pts[0] - last.pos;
                            if last.kind == PathPointKind::Corner
                                && (last.in_tan.length_squared() > 1e-10
                                    || last.out_tan.length_squared() > 1e-10)
                            {
                                last.kind = PathPointKind::Smooth;
                            }
                        }
                        let mut node = PathPoint::corner(pts[2]);
                        node.in_tan = pts[1] - pts[2];
                        node.kind = PathPointKind::Smooth;
                        path.points.push(node);
                        cursor = pts[2];
                    }
                }
                'Q' => {
                    while tokens.get(i).and_then(|t| t.as_num()).is_some() {
                        let vals: Vec<f32> =
                            (0..4).map(|_| num(&tokens, &mut i).unwrap_or(0.0)).collect();
                        let mut ctrl = Vec2::new(vals[0], vals[1]);
                        let mut end = Vec2::new(vals[2], vals[3]);
                        if rel {
                            ctrl = ctrl + cursor;
                            end = end + cursor;
                        }
                        // Degree-elevate quadratic to cubic.
                        let c1 = cursor + (ctrl - cursor) * (2.0 / 3.0);
                        let c2 = end + (ctrl - end) * (2.0 / 3.0);
                        if let Some(last) = path.points.last_mut() {
                            last.out_tan = c1 - last.pos;
                            if last.kind == PathPointKind::Corner {
                                last.kind = PathPointKind::Smooth;
                            }
                        }
                        let mut node = PathPoint::corner(end);
                        node.in_tan = c2 - end;
                        node.kind = PathPointKind::Smooth;
                        path.points.push(node);
                        cursor = end;
                    }
                }
                'Z' => {
                    path.closed = true;
                }
                _ => break,
            }
        }
        // A final C/L ending exactly on the start node duplicates it; drop
        // the copy so closed paths keep clean topology for morphing.
        if path.closed && path.points.len() > 1 {
            let (first, last) = (path.points[0].pos, path.points[path.points.len() - 1].pos);
            if first.distance_to(last) < 1e-4 {
                path.points.pop();
            }
        }
        path
    }

    /// Serialize to the SVG subset (`M/L/C/Z`).
    pub fn to_svg(&self) -> String {
        if self.points.is_empty() {
            return String::new();
        }
        let mut s = String::new();
        let f = |v: f32| {
            if (v - v.round()).abs() < 1e-4 {
                format!("{}", v.round() as i64)
            } else {
                format!("{v:.2}")
            }
        };
        // Emit move + per-node cubic segments (lines degenerate to C).
        let p0 = self.points[0].pos;
        s.push_str(&format!("M {} {}", f(p0.x), f(p0.y)));
        for i in 0..self.segment_count() {
            if let Some((_, c1, c2, p)) = self.segment(i) {
                s.push_str(&format!(" C {} {} {} {} {} {}", f(c1.x), f(c1.y), f(c2.x), f(c2.y), f(p.x), f(p.y)));
            }
        }
        if self.closed {
            s.push_str(" Z");
        }
        s
    }
}

impl Interpolate for Path {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        // Morph requires identical topology; otherwise hold/step.
        if self.points.len() != other.points.len() || self.closed != other.closed {
            return self.step(other, t);
        }
        Self {
            points: self
                .points
                .iter()
                .zip(other.points.iter())
                .map(|(a, b)| a.lerp(b, t))
                .collect(),
            closed: self.closed,
        }
    }
}

/// Boolean operation selector for [`Path::boolean_op`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathBooleanOp {
    Union,
    Intersect,
    Subtract,
    Difference,
    Xor,
}

// --- Flattening / polyline helpers ---

fn flatten_cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, tol: f32, out: &mut Vec<Vec2>) {
    // Pure lines need no subdivision (also keeps rect edges crisp for GH).
    if p1.distance_to(p0) < 1e-6 && p2.distance_to(p3) < 1e-6 {
        return;
    }
    // Adaptive subdivision on control-hull flatness.
    let ux = 3.0 * p1.x - 2.0 * p0.x - p3.x;
    let uy = 3.0 * p1.y - 2.0 * p0.y - p3.y;
    let vx = 3.0 * p2.x - 2.0 * p3.x - p0.x;
    let vy = 3.0 * p2.y - 2.0 * p3.y - p0.y;
    let (ux, uy, vx, vy) = (ux * ux, uy * uy, vx * vx, vy * vy);
    if ux + vx < 16.0 * tol * tol && uy + vy < 16.0 * tol * tol {
        return;
    }
    let m01 = (p0 + p1) * 0.5;
    let m12 = (p1 + p2) * 0.5;
    let m23 = (p2 + p3) * 0.5;
    let m012 = (m01 + m12) * 0.5;
    let m123 = (m12 + m23) * 0.5;
    let mid = (m012 + m123) * 0.5;
    flatten_cubic(p0, m01, m012, mid, tol, out);
    out.push(mid);
    flatten_cubic(mid, m123, m23, p3, tol, out);
}

fn poly_length(pts: &[Vec2], closed: bool) -> f32 {
    if pts.len() < 2 {
        return 0.0;
    }
    let mut len = 0.0;
    for i in 0..pts.len() - 1 {
        len += pts[i].distance_to(pts[i + 1]);
    }
    if closed {
        len += pts[pts.len() - 1].distance_to(pts[0]);
    }
    len
}

fn point_at_poly_ratio(pts: &[Vec2], closed: bool, t: f32) -> Option<Vec2> {
    let total = poly_length(pts, closed);
    if total < 1e-9 {
        return pts.first().copied();
    }
    let mut target = t.clamp(0.0, 1.0) * total;
    let segs = if closed { pts.len() } else { pts.len() - 1 };
    for i in 0..segs {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let l = a.distance_to(b);
        if target <= l {
            let f = if l < 1e-9 { 0.0 } else { target / l };
            return Some(a.lerp(b, f));
        }
        target -= l;
    }
    Some(if closed { pts[0] } else { pts[pts.len() - 1] })
}

fn rdp(pts: &[Vec2], first: usize, last: usize, tol: f32, keep: &mut [bool]) {
    if last <= first + 1 {
        return;
    }
    let (a, b) = (pts[first], pts[last]);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let denom = (dx * dx + dy * dy).sqrt().max(1e-9);
    let mut max_d = 0.0f32;
    let mut idx = first;
    for i in (first + 1)..last {
        let d = ((pts[i].x - a.x) * dy - (pts[i].y - a.y) * dx).abs() / denom;
        if d > max_d {
            max_d = d;
            idx = i;
        }
    }
    if max_d > tol {
        keep[idx] = true;
        rdp(pts, first, idx, tol, keep);
        rdp(pts, idx, last, tol, keep);
    }
}

// --- SVG tokenizer ---

#[derive(Debug, Clone, Copy)]
enum SvgTok {
    Cmd(char),
    Num(f32),
}

impl SvgTok {
    fn as_num(&self) -> Option<f32> {
        match self {
            SvgTok::Num(v) => Some(*v),
            _ => None,
        }
    }
}

fn tokenize_svg(data: &str) -> Vec<SvgTok> {
    let mut out = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, out: &mut Vec<SvgTok>| {
        if !num.is_empty() {
            if let Ok(v) = num.parse::<f32>() {
                out.push(SvgTok::Num(v));
            }
            num.clear();
        }
    };
    let mut chars = data.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_alphabetic() {
            flush(&mut num, &mut out);
            out.push(SvgTok::Cmd(c));
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            // A sign after digits starts a new number ("10-20" idiom).
            if (c == '-' || c == '+') && !num.is_empty() && !num.ends_with('e') && !num.ends_with('E') {
                flush(&mut num, &mut out);
            }
            num.push(c);
        } else if c == ',' || c.is_whitespace() {
            flush(&mut num, &mut out);
        }
    }
    flush(&mut num, &mut out);
    out
}

// --- Polygon booleans (Greiner–Hormann on flattened contours) ---

#[derive(Clone, Copy)]
struct GhVertex {
    p: Vec2,
    /// Intersection bookkeeping.
    entry: bool,
    intersect: bool,
}

fn seg_intersection(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32, Vec2)> {
    let r = b - a;
    let s = d - c;
    let denom = r.x * s.y - r.y * s.x;
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = ((c.x - a.x) * s.y - (c.y - a.y) * s.x) / denom;
    let u = ((c.x - a.x) * r.y - (c.y - a.y) * r.x) / denom;
    // Strict interior crossings only; shared endpoints are degenerate for
    // GH and handled by the containment fallback instead.
    const EPS: f32 = 1e-6;
    if t > EPS && t < 1.0 - EPS && u > EPS && u < 1.0 - EPS {
        Some((t, u, a + r * t))
    } else {
        None
    }
}

fn point_in_poly(p: Vec2, poly: &[Vec2]) -> bool {
    // Even-odd ray cast.
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[j], poly[i]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y).max(1e-12) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn signed_area(poly: &[Vec2]) -> f32 {
    let mut a = 0.0;
    for i in 0..poly.len() {
        let b = poly[(i + 1) % poly.len()];
        a += poly[i].x * b.y - b.x * poly[i].y;
    }
    a * 0.5
}

/// Greiner–Hormann boolean on two simple polygons. Returns result contours.
/// Degenerate inputs (touching vertices, overlaps) fall back to
/// containment logic, which is exact for disjoint/contained cases.
pub fn boolean_polygons(a: &[Vec2], b: &[Vec2], op: PathBooleanOp) -> Vec<Vec<Vec2>> {
    let clean = |p: &[Vec2]| -> Vec<Vec2> {
        // Drop consecutive duplicates (break GH intersection sorting).
        let mut out = Vec::with_capacity(p.len());
        for v in p {
            if out.last().map(|l: &Vec2| l.distance_to(*v) > 1e-6).unwrap_or(true) {
                out.push(*v);
            }
        }
        if out.len() > 1 && out[0].distance_to(out[out.len() - 1]) < 1e-6 {
            out.pop();
        }
        out
    };
    let (pa, pb) = (clean(a), clean(b));
    if pa.len() < 3 || pb.len() < 3 {
        return Vec::new();
    }
    // Intersections per edge pair.
    #[derive(Clone, Debug)]
    struct Xing {
        ai: usize,
        at: f32,
        bi: usize,
        bt: f32,
        p: Vec2,
    }
    let mut xings: Vec<Xing> = Vec::new();
    for i in 0..pa.len() {
        let (a0, a1) = (pa[i], pa[(i + 1) % pa.len()]);
        for j in 0..pb.len() {
            let (b0, b1) = (pb[j], pb[(j + 1) % pb.len()]);
            if let Some((t, u, p)) = seg_intersection(a0, a1, b0, b1) {
                xings.push(Xing { ai: i, at: t, bi: j, bt: u, p });
            }
        }
    }
    // No crossings: pure containment / disjoint cases (exact).
    if xings.is_empty() {
        let a_in_b = point_in_poly(pa[0], &pb);
        let b_in_a = point_in_poly(pb[0], &pa);
        return match op {
            PathBooleanOp::Union => {
                if a_in_b {
                    vec![pb]
                } else if b_in_a {
                    vec![pa]
                } else {
                    vec![pa, pb]
                }
            }
            PathBooleanOp::Intersect => {
                if a_in_b {
                    vec![pa]
                } else if b_in_a {
                    vec![pb]
                } else {
                    Vec::new()
                }
            }
            PathBooleanOp::Subtract | PathBooleanOp::Difference => {
                if b_in_a {
                    // Hole case: even-odd fill renders holes correctly when
                    // both contours share one path.
                    vec![pa, pb]
                } else if a_in_b {
                    Vec::new()
                } else {
                    vec![pa]
                }
            }
            PathBooleanOp::Xor => {
                if a_in_b {
                    vec![pb, pa]
                } else if b_in_a {
                    vec![pa, pb]
                } else {
                    vec![pa, pb]
                }
            }
        };
    }
    // Normalize winding (CCW) so traversal rules hold; entry flags are
    // orientation-independent (inside/outside tests).
    let mut pa = pa;
    let mut pb = pb;
    if signed_area(&pa) < 0.0 {
        pa.reverse();
    }
    if signed_area(&pb) < 0.0 {
        pb.reverse();
    }
    // Rebuild enriched rings with sorted intersections.
    let mut ra: Vec<GhVertex> = pa
        .iter()
        .map(|&p| GhVertex { p, entry: false, intersect: false })
        .collect();
    let mut rb: Vec<GhVertex> = pb
        .iter()
        .map(|&p| GhVertex { p, entry: false, intersect: false })
        .collect();
    // Group crossings per edge, sorted by t.
    let mut per_a: Vec<Vec<usize>> = vec![Vec::new(); pa.len()];
    let mut per_b: Vec<Vec<usize>> = vec![Vec::new(); pb.len()];
    for (k, x) in xings.iter().enumerate() {
        per_a[x.ai].push(k);
        per_b[x.bi].push(k);
    }
    for list in per_a.iter_mut() {
        list.sort_by(|&i, &j| xings[i].at.partial_cmp(&xings[j].at).unwrap());
    }
    for list in per_b.iter_mut() {
        list.sort_by(|&i, &j| xings[i].bt.partial_cmp(&xings[j].bt).unwrap());
    }
    // Insert intersection vertices (from last edge backward to keep indices).
    for i in (0..pa.len()).rev() {
        for &k in per_a[i].iter().rev() {
            ra.insert(
                i + 1,
                GhVertex { p: xings[k].p, entry: false, intersect: true },
            );
        }
    }
    for j in (0..pb.len()).rev() {
        for &k in per_b[j].iter().rev() {
            rb.insert(
                j + 1,
                GhVertex { p: xings[k].p, entry: false, intersect: true },
            );
        }
    }
    // Link intersection twins (match by position, tolerant).
    let mut a_twins = vec![usize::MAX; ra.len()];
    let mut b_twins = vec![usize::MAX; rb.len()];
    for (ia, va) in ra.iter().enumerate() {
        if !va.intersect {
            continue;
        }
        for (ib, vb) in rb.iter().enumerate() {
            if !vb.intersect || b_twins[ib] != usize::MAX {
                continue;
            }
            if va.p.distance_to(vb.p) < 1e-4 {
                a_twins[ia] = ib;
                b_twins[ib] = ia;
                break;
            }
        }
    }
    // Every crossing must twin up (non-crossing vertices never match).
    let linked = ra
        .iter()
        .enumerate()
        .filter(|(_, v)| v.intersect)
        .all(|(ia, _)| a_twins[ia] != usize::MAX);
    if !linked {
        // Degenerate linking: fall back to containment handling.
        return boolean_polygons_fallback(&pa, &pb, op);
    }
    // Entry flags: the edge AFTER the crossing lies inside the other ring.
    let mark_entries = |ring: &mut Vec<GhVertex>, other: &[Vec2]| {
        let n = ring.len();
        for i in 0..n {
            if !ring[i].intersect {
                continue;
            }
            let nxt = ring[(i + 1) % n].p;
            let mid = (ring[i].p + nxt) * 0.5;
            ring[i].entry = point_in_poly(mid, other);
        }
    };
    mark_entries(&mut ra, &pb);
    mark_entries(&mut rb, &pa);
    // Standard Greiner–Hormann traversal (entry = forward edge after the
    // crossing lies inside the other ring; CCW/CCW gives A-exit ⟺ B-entry).
    // - Intersect: follow inside edges (switch at exits), start A-entry.
    // - Union: follow outside edges (switch at entries), start A-exit.
    // - Subtract (A−B): A forward outside edges + B inside edges run
    //   backward; switch at entries on both sides; start A-exit.
    let (switch_on_entry, b_backward) = match op {
        PathBooleanOp::Intersect => (false, false),
        PathBooleanOp::Union | PathBooleanOp::Subtract | PathBooleanOp::Difference => (true, matches!(op, PathBooleanOp::Subtract | PathBooleanOp::Difference)),
        PathBooleanOp::Xor => (false, false),
    };
    let start_wants_entry = matches!(op, PathBooleanOp::Intersect);
    let mut used_a = vec![false; ra.len()];
    let mut used_b = vec![false; rb.len()];
    let mut results: Vec<Vec<Vec2>> = Vec::new();
    // XOR via union + intersection shells (even-odd consumers pair them).
    if op == PathBooleanOp::Xor {
        let mut u = boolean_polygons(&pa, &pb, PathBooleanOp::Union);
        let i = boolean_polygons(&pa, &pb, PathBooleanOp::Intersect);
        u.extend(i);
        u.retain(|c| c.len() > 2 && polygon_area(c).abs() > 1e-6);
        return u;
    }
    let starts: Vec<usize> = ra
        .iter()
        .enumerate()
        .filter(|(_, v)| v.intersect && v.entry == start_wants_entry)
        .map(|(i, _)| i)
        .collect();
    for start_idx in starts {
        if used_a[start_idx] {
            continue;
        }
        let mut contour = Vec::new();
        // (on_a, index) plus backward flag while on B for subtract.
        let (mut on_a, mut idx) = (true, start_idx);
        let mut on_b_backward = false;
        loop {
            if on_a {
                if used_a[idx] {
                    break;
                }
                used_a[idx] = true;
                contour.push(ra[idx].p);
                if ra[idx].intersect {
                    let is_exit = !ra[idx].entry;
                    let should_switch = if switch_on_entry { ra[idx].entry } else { is_exit };
                    if should_switch {
                        let twin = a_twins[idx];
                        on_a = false;
                        on_b_backward = b_backward;
                        idx = twin;
                        continue;
                    }
                }
                idx = (idx + 1) % ra.len();
            } else if on_b_backward {
                if used_b[idx] {
                    break;
                }
                used_b[idx] = true;
                contour.push(rb[idx].p);
                if rb[idx].intersect {
                    let is_exit = !rb[idx].entry;
                    let should_switch = if switch_on_entry { rb[idx].entry } else { is_exit };
                    if should_switch {
                        let twin = b_twins[idx];
                        on_a = true;
                        on_b_backward = false;
                        idx = twin;
                        continue;
                    }
                }
                idx = (idx + rb.len() - 1) % rb.len();
            } else {
                if used_b[idx] {
                    break;
                }
                used_b[idx] = true;
                contour.push(rb[idx].p);
                if rb[idx].intersect {
                    let is_exit = !rb[idx].entry;
                    let should_switch = if switch_on_entry { rb[idx].entry } else { is_exit };
                    if should_switch {
                        let twin = b_twins[idx];
                        on_a = true;
                        idx = twin;
                        continue;
                    }
                }
                idx = (idx + 1) % rb.len();
            }
            if contour.len() > ra.len() + rb.len() + 4 {
                break;
            }
        }
        if contour.len() > 2 {
            results.push(contour);
        }
    }
    if results.is_empty() {
        return boolean_polygons_fallback(&pa, &pb, op);
    }
    results.retain(|c| c.len() > 2 && polygon_area(c).abs() > 1e-6);
    if results.is_empty() {
        return boolean_polygons_fallback(&pa, &pb, op);
    }
    results
}

fn polygon_area(poly: &[Vec2]) -> f32 {
    signed_area(poly)
}

fn boolean_polygons_fallback(a: &[Vec2], b: &[Vec2], op: PathBooleanOp) -> Vec<Vec<Vec2>> {    let a_in_b = point_in_poly(a[0], b);
    let b_in_a = point_in_poly(b[0], a);
    match op {
        PathBooleanOp::Union => {
            if a_in_b {
                vec![b.to_vec()]
            } else if b_in_a {
                vec![a.to_vec()]
            } else {
                vec![a.to_vec(), b.to_vec()]
            }
        }
        PathBooleanOp::Intersect => {
            if a_in_b {
                vec![a.to_vec()]
            } else if b_in_a {
                vec![b.to_vec()]
            } else {
                Vec::new()
            }
        }
        PathBooleanOp::Subtract | PathBooleanOp::Difference => {
            if b_in_a {
                vec![a.to_vec(), b.to_vec()]
            } else if a_in_b {
                Vec::new()
            } else {
                vec![a.to_vec()]
            }
        }
        PathBooleanOp::Xor => {
            if a_in_b || b_in_a {
                vec![a.to_vec(), b.to_vec()]
            } else {
                vec![a.to_vec(), b.to_vec()]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_sum(polys: &[Vec<Vec2>]) -> f32 {
        polys.iter().map(|p| polygon_area(p).abs()).sum()
    }

    #[test]
    fn rect_path_roundtrips_svg() {
        let p = Path::rectangle(10.0, 20.0, 100.0, 50.0);
        assert_eq!(p.segment_count(), 4);
        let svg = p.to_svg();
        let q = Path::from_svg(&svg);
        assert_eq!(q.points.len(), 4);
        assert!(q.closed);
        let (mn, mx) = q.bounds().unwrap();
        assert!((mn.x - 10.0).abs() < 1e-3 && (mx.x - 110.0).abs() < 1e-3);
        assert!((mn.y - 20.0).abs() < 1e-3 && (mx.y - 70.0).abs() < 1e-3);
    }

    #[test]
    fn svg_subset_parses() {
        let p = Path::from_svg("M 0 0 L 10 0 L 10 10 Z");
        assert_eq!(p.points.len(), 3);
        assert!(p.closed);
        let q = Path::from_svg("m 5 5 l 10 0 l 0 10 z");
        assert_eq!(q.points.len(), 3);
        assert!((q.points[1].pos.x - 15.0).abs() < 1e-4);
        let c = Path::from_svg("M 0 0 C 10 0 10 10 20 10");
        assert_eq!(c.points.len(), 2);
        assert!(c.points[0].out_tan.length() > 1.0);
    }

    #[test]
    fn smooth_symmetric_coupling() {
        let mut p = PathPoint::smooth(Vec2::ZERO, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0));
        p.set_in_abs(Vec2::new(0.0, -20.0));
        // Smooth keeps the other length (10) but mirrors direction.
        assert!((p.out_tan.length() - 10.0).abs() < 1e-4);
        assert!(p.out_tan.y > 0.0);
        let mut q = PathPoint::smooth(Vec2::ZERO, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0));
        q.convert_to(PathPointKind::Symmetric);
        q.set_out_abs(Vec2::new(6.0, 8.0));
        assert!((q.in_tan.x + 6.0).abs() < 1e-4 && (q.in_tan.y + 8.0).abs() < 1e-4);
    }

    #[test]
    fn reverse_split_morph() {
        let p = Path::rectangle(0.0, 0.0, 40.0, 40.0);
        let r = p.reversed();
        assert_eq!(r.points.len(), 4);
        assert!((r.bounds().unwrap().1.x - 40.0).abs() < 1e-3);
        let mut s = p.clone();
        let idx = s.split_segment(0, 0.5).unwrap();
        assert_eq!(s.points.len(), 5);
        assert!((s.points[idx].pos.x - 20.0).abs() < 1e-3);
        assert!((s.points[idx].pos.y - 0.0).abs() < 1e-3);
        // Morph mid-way between two rects.
        let a = Path::rectangle(0.0, 0.0, 10.0, 10.0);
        let b = Path::rectangle(0.0, 0.0, 20.0, 20.0);
        let m = a.lerp(&b, 0.5);
        assert!((m.bounds().unwrap().1.x - 15.0).abs() < 1e-3);
        // Topology mismatch holds.
        let tri = Path { points: vec![PathPoint::corner(Vec2::ZERO); 3], closed: true };
        assert_eq!(a.lerp(&tri, 0.5).points.len(), 4);
    }

    #[test]
    fn length_and_point_at_ratio() {
        let p = Path::rectangle(0.0, 0.0, 40.0, 30.0);
        assert!((p.length(0.5) - 140.0).abs() < 2.0);
        let mid = p.point_at_ratio(0.25, 0.5).unwrap();
        assert!((mid.x - 35.0).abs() < 2.0 && mid.y.abs() < 2.0);
        // Open path ends.
        let mut o = Path::new();
        o.line_to(Vec2::ZERO);
        o.line_to(Vec2::new(10.0, 0.0));
        assert_eq!(o.point_at_ratio(0.0, 0.1).unwrap(), Vec2::ZERO);
        assert_eq!(o.point_at_ratio(1.0, 0.1).unwrap(), Vec2::new(10.0, 0.0));
    }

    #[test]
    fn simplify_and_offset() {
        let mut p = Path::new();
        for i in 0..21 {
            p.line_to(Vec2::new(i as f32 * 5.0, if i % 2 == 0 { 0.0 } else { 0.5 }));
        }
        let s = p.simplify(1.0);
        assert!(s.points.len() < p.points.len());
        assert!(s.points.len() >= 2);
        let r = Path::rectangle(0.0, 0.0, 40.0, 40.0);
        let o = r.offset(5.0, 8);
        let (mn, mx) = o.bounds().unwrap();
        assert!(mn.x < -2.0 && mx.x > 42.0, "{mn:?} {mx:?}");
    }

    #[test]
    fn boolean_rect_overlap() {
        let a = Path::rectangle(0.0, 0.0, 40.0, 40.0);
        let b = Path::rectangle(20.0, 20.0, 40.0, 40.0);
        // Union area = 1600 + 1600 - 400 = 2800.
        let u = a.boolean_op(&b, PathBooleanOp::Union, 0.5);
        let areas: Vec<f32> = u.iter().map(|p| polygon_area(&p.flatten(0.5)).abs()).collect();
            assert!((area_sum(&u.iter().map(|p| p.flatten(0.5)).collect::<Vec<_>>()) - 2800.0).abs() < 60.0, "{u:?}");
        // Intersect = 20x20 = 400.
        let i = a.boolean_op(&b, PathBooleanOp::Intersect, 0.5);
        assert!((area_sum(&i.iter().map(|p| p.flatten(0.5)).collect::<Vec<_>>()) - 400.0).abs() < 40.0, "{i:?}");
        // A-B = 1600 - 400 = 1200.
        let s = a.boolean_op(&b, PathBooleanOp::Subtract, 0.5);
        assert!((area_sum(&s.iter().map(|p| p.flatten(0.5)).collect::<Vec<_>>()) - 1200.0).abs() < 60.0, "{s:?}");
    }

    #[test]
    fn boolean_containment_and_disjoint() {
        let outer = Path::rectangle(0.0, 0.0, 100.0, 100.0);
        let inner = Path::rectangle(25.0, 25.0, 50.0, 50.0);
        let far = Path::rectangle(200.0, 200.0, 10.0, 10.0);
        assert_eq!(outer.boolean_op(&far, PathBooleanOp::Union, 0.5).len(), 2);
        assert!(outer.boolean_op(&far, PathBooleanOp::Intersect, 0.5).is_empty());
        assert_eq!(outer.boolean_op(&inner, PathBooleanOp::Union, 0.5).len(), 1);
        let sub = outer.boolean_op(&inner, PathBooleanOp::Subtract, 0.5);
        // Hole pair for even-odd rasterization.
        assert_eq!(sub.len(), 2);
        assert!(outer.boolean_op(&far, PathBooleanOp::Subtract, 0.5).len() >= 1);
    }
    #[test]
    fn transform_applies_to_handles() {
        let e = Path::ellipse(0.0, 0.0, 10.0, 5.0);
        let t = e.transformed(2.0, 0.0, 0.0, 2.0, 5.0, 5.0);
        let (mn, mx) = t.bounds().unwrap();
        assert!((mn.x + 15.0).abs() < 0.5 && (mx.x - 25.0).abs() < 0.5);
        assert!((mn.y + 5.0).abs() < 0.5 && (mx.y - 15.0).abs() < 0.5);
    }

    #[test]
    fn tangent_follows_direction() {
        let mut p = Path::new();
        p.line_to(Vec2::new(0.0, 0.0));
        p.line_to(Vec2::new(40.0, 0.0));
        // East: 0 degrees.
        assert!((p.tangent_at_ratio(0.5, 0.1).unwrap() - 0.0).abs() < 2.0);
        let mut q = Path::new();
        q.line_to(Vec2::new(0.0, 0.0));
        q.line_to(Vec2::new(0.0, 30.0));
        // South in screen coords: +90 degrees.
        assert!((q.tangent_at_ratio(0.5, 0.1).unwrap() - 90.0).abs() < 2.0);
        // Degenerate path has no tangent.
        let mut single = Path::new();
        single.line_to(Vec2::new(3.0, 4.0));
        assert!(single.tangent_at_ratio(0.5, 0.1).is_none());
    }
}
