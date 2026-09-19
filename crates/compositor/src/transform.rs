use crate::error::SceneGraphError;
use crate::graph::SceneGraph;
use project::{TimeCode, Transform, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::{Mul, MulAssign};

/// A 2D affine transformation matrix for motion graphics compositing.
///
/// In standard 2D homogeneous screen coordinates (where x extends to the right
/// and y extends downward), an affine transform is represented by the 3x3 matrix:
/// ```text
/// [ a   c   tx ]   [ x ]   [ a*x + c*y + tx ]
/// [ b   d   ty ] * [ y ] = [ b*x + d*y + ty ]
/// [ 0   0   1  ]   [ 1 ]   [       1        ]
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffineTransform2D {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl AffineTransform2D {
    /// The identity transformation matrix.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// Construct an affine transform from raw 6 coefficients.
    pub const fn new(a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) -> Self {
        Self { a, b, c, d, tx, ty }
    }

    /// Construct a pure translation transformation.
    pub const fn from_translation(offset: Vec2) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: offset.x,
            ty: offset.y,
        }
    }

    /// Construct a non-uniform scale transformation.
    pub const fn from_scale(scale: Vec2) -> Self {
        Self {
            a: scale.x,
            b: 0.0,
            c: 0.0,
            d: scale.y,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Construct a uniform scale transformation.
    pub const fn from_scale_uniform(scale: f32) -> Self {
        Self::from_scale(Vec2::splat(scale))
    }

    /// Construct a rotation transformation from radians (clockwise in screen coordinates).
    pub fn from_rotation_radians(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Construct a rotation transformation from degrees (clockwise in screen coordinates).
    pub fn from_rotation_degrees(degrees: f32) -> Self {
        Self::from_rotation_radians(degrees.to_radians())
    }

    /// Construct a complete local transform matrix from After Effects-style components:
    ///
    /// `M_local = T(position) * R(rotation) * S(scale / 100.0) * T(-anchor_point)`
    ///
    /// This ensures that:
    /// 1. The layer's anchor point is placed at `position` in the target coordinate space.
    /// 2. Rotation and scaling occur around the anchor point.
    pub fn from_transform_components(
        position: Vec2,
        scale_percent: Vec2,
        rotation_degrees: f32,
        anchor_point: Vec2,
    ) -> Self {
        let t_pos = Self::from_translation(position);
        let r = Self::from_rotation_degrees(rotation_degrees);
        let s = Self::from_scale(scale_percent / 100.0);
        let t_neg_anchor = Self::from_translation(-anchor_point);

        t_pos * r * s * t_neg_anchor
    }

    /// Construct a local transform matrix from a `project::Transform`.
    pub fn from_transform(transform: &Transform) -> Self {
        Self::from_transform_components(
            *transform.position.value(),
            *transform.scale.value(),
            *transform.rotation.value(),
            *transform.anchor_point.value(),
        )
    }

    /// Check if all components of this transform are finite (not NaN or infinite).
    pub fn is_finite(&self) -> bool {
        self.a.is_finite()
            && self.b.is_finite()
            && self.c.is_finite()
            && self.d.is_finite()
            && self.tx.is_finite()
            && self.ty.is_finite()
    }

    /// Calculate the determinant of the 2x2 linear portion of the transform (`a*d - b*c`).
    pub fn determinant(&self) -> f32 {
        self.a * self.d - self.b * self.c
    }

    /// Return true if this transform is invertible (determinant is finite and non-zero).
    pub fn is_invertible(&self) -> bool {
        if !self.is_finite() {
            return false;
        }
        let det = self.determinant();
        det.is_finite() && det.abs() > 1e-6
    }

    /// Compute the inverse affine transformation, or return `None` if non-invertible.
    pub fn inverse(&self) -> Option<Self> {
        if !self.is_finite() {
            return None;
        }
        let det = self.determinant();
        if !det.is_finite() || det.abs() <= 1e-6 {
            return None;
        }

        let inv_det = 1.0 / det;
        let inv = Self {
            a: self.d * inv_det,
            b: -self.b * inv_det,
            c: -self.c * inv_det,
            d: self.a * inv_det,
            tx: (self.c * self.ty - self.d * self.tx) * inv_det,
            ty: (self.b * self.tx - self.a * self.ty) * inv_det,
        };

        if inv.is_finite() {
            Some(inv)
        } else {
            None
        }
    }

    /// Transform a 2D point (applying linear transformation and translation).
    pub fn transform_point(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }

    /// Transform a 2D point by the inverse transform, returning `None` if non-invertible.
    pub fn transform_point_inverse(&self, p: Vec2) -> Option<Vec2> {
        let inv = self.inverse()?;
        Some(inv.transform_point(p))
    }

    /// Transform a 2D vector (direction/offset without translation).
    pub fn transform_vector(&self, v: Vec2) -> Vec2 {
        Vec2::new(
            self.a * v.x + self.c * v.y,
            self.b * v.x + self.d * v.y,
        )
    }

    /// Convert to a 3x3 matrix in row-major order: `[[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]]`.
    pub fn to_matrix_3x3(&self) -> [[f32; 3]; 3] {
        [
            [self.a, self.c, self.tx],
            [self.b, self.d, self.ty],
            [0.0, 0.0, 1.0],
        ]
    }

    /// Convert to a 3x3 matrix in column-major order: `[[c00, c10, c20], [c01, c11, c21], [c02, c12, c22]]`.
    /// Suitable for GPU / shader buffer uploads.
    pub fn to_matrix_3x3_cols(&self) -> [[f32; 3]; 3] {
        [
            [self.a, self.b, 0.0],
            [self.c, self.d, 0.0],
            [self.tx, self.ty, 1.0],
        ]
    }

    /// Convert to a flat array of 9 elements in row-major order.
    pub fn to_matrix_3x3_flat(&self) -> [f32; 9] {
        [
            self.a, self.c, self.tx,
            self.b, self.d, self.ty,
            0.0, 0.0, 1.0,
        ]
    }

    /// Construct an affine transform from a 3x3 matrix in row-major order.
    pub fn from_matrix_3x3(m: [[f32; 3]; 3]) -> Self {
        Self {
            a: m[0][0],
            c: m[0][1],
            tx: m[0][2],
            b: m[1][0],
            d: m[1][1],
            ty: m[1][2],
        }
    }

    /// Construct an affine transform from a flat array of 9 elements in row-major order.
    pub fn from_matrix_3x3_flat(m: [f32; 9]) -> Self {
        Self {
            a: m[0],
            c: m[1],
            tx: m[2],
            b: m[3],
            d: m[4],
            ty: m[5],
        }
    }

    /// Convert to a 6-element column-major array `[a, b, c, d, tx, ty]`.
    pub const fn to_cols_array(&self) -> [f32; 6] {
        [self.a, self.b, self.c, self.d, self.tx, self.ty]
    }

    /// Construct an affine transform from a 6-element column-major array `[a, b, c, d, tx, ty]`.
    pub const fn from_cols_array(m: [f32; 6]) -> Self {
        Self {
            a: m[0],
            b: m[1],
            c: m[2],
            d: m[3],
            tx: m[4],
            ty: m[5],
        }
    }

    /// Check if two transforms are approximately equal within a given tolerance.
    pub fn approx_eq(&self, other: &Self, epsilon: f32) -> bool {
        (self.a - other.a).abs() <= epsilon
            && (self.b - other.b).abs() <= epsilon
            && (self.c - other.c).abs() <= epsilon
            && (self.d - other.d).abs() <= epsilon
            && (self.tx - other.tx).abs() <= epsilon
            && (self.ty - other.ty).abs() <= epsilon
    }
}

impl Default for AffineTransform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for AffineTransform2D {
    type Output = Self;

    /// Concatenate two affine transforms: `(self * rhs)`.
    ///
    /// When transforming a point `P`, `(M1 * M2) * P == M1 * (M2 * P)`.
    /// In other words, `rhs` is applied first, followed by `self`.
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            tx: self.a * rhs.tx + self.c * rhs.ty + self.tx,
            ty: self.b * rhs.tx + self.d * rhs.ty + self.ty,
        }
    }
}

impl MulAssign for AffineTransform2D {
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl Mul<Vec2> for AffineTransform2D {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        self.transform_point(rhs)
    }
}

/// A 2D axis-aligned bounding box (AABB) defined by minimum and maximum coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox2D {
    pub min: Vec2,
    pub max: Vec2,
}

impl BoundingBox2D {
    /// Zero bounding box.
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        max: Vec2::ZERO,
    };

    /// Create a bounding box from minimum and maximum coordinates, ensuring min <= max.
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self {
            min: Vec2::new(min.x.min(max.x), min.y.min(max.y)),
            max: Vec2::new(min.x.max(max.x), min.y.max(max.y)),
        }
    }

    /// Create a bounding box from top-left origin and size.
    pub fn from_origin_size(origin: Vec2, size: Vec2) -> Self {
        Self::new(origin, origin + size)
    }

    /// Create a bounding box centered at `center` with dimensions `size`.
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = size * 0.5;
        Self::new(center - half, center + half)
    }

    /// Create a bounding box enclosing an iterator of 2D points.
    pub fn from_points<I: IntoIterator<Item = Vec2>>(points: I) -> Option<Self> {
        let mut iter = points.into_iter();
        let first = iter.next()?;
        let mut min = first;
        let mut max = first;
        for p in iter {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        Some(Self { min, max })
    }

    /// Return width of the bounding box.
    pub fn width(&self) -> f32 {
        (self.max.x - self.min.x).max(0.0)
    }

    /// Return height of the bounding box.
    pub fn height(&self) -> f32 {
        (self.max.y - self.min.y).max(0.0)
    }

    /// Return dimensions of the bounding box as a `Vec2`.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width(), self.height())
    }

    /// Return area of the bounding box.
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Return true if the bounding box has zero or negative width or height.
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// Return center point of the bounding box.
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// Return the four corner vertices in clockwise order starting from top-left.
    pub fn corners(&self) -> [Vec2; 4] {
        [
            Vec2::new(self.min.x, self.min.y),
            Vec2::new(self.max.x, self.min.y),
            Vec2::new(self.max.x, self.max.y),
            Vec2::new(self.min.x, self.max.y),
        ]
    }

    /// Check if a point lies inside this bounding box (inclusive).
    pub fn contains_point(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Check if this bounding box intersects another bounding box.
    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// Compute the intersection of two bounding boxes, returning `None` if they do not overlap.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.intersects(other) {
            return None;
        }
        let min_x = self.min.x.max(other.min.x);
        let min_y = self.min.y.max(other.min.y);
        let max_x = self.max.x.min(other.max.x);
        let max_y = self.max.y.min(other.max.y);
        if min_x > max_x || min_y > max_y {
            return None;
        }
        Some(Self {
            min: Vec2::new(min_x, min_y),
            max: Vec2::new(max_x, max_y),
        })
    }

    /// Compute the union bounding box that encloses both bounding boxes.
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// Transform this bounding box by an affine transform, returning the enclosing axis-aligned bounding box.
    pub fn transform(&self, matrix: &AffineTransform2D) -> Self {
        let corners = self.corners();
        let transformed = [
            matrix.transform_point(corners[0]),
            matrix.transform_point(corners[1]),
            matrix.transform_point(corners[2]),
            matrix.transform_point(corners[3]),
        ];
        Self::from_points(transformed).unwrap_or(Self::ZERO)
    }

    /// Transform this bounding box by the inverse of an affine transform, returning `None` if non-invertible.
    pub fn transform_inverse(&self, matrix: &AffineTransform2D) -> Option<Self> {
        let inv = matrix.inverse()?;
        Some(self.transform(&inv))
    }
}

impl Default for BoundingBox2D {
    fn default() -> Self {
        Self::ZERO
    }
}

/// The evaluated spatial transform state for a layer, holding both local and world transforms
/// along with individual source components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EvaluatedTransform {
    pub local_matrix: AffineTransform2D,
    pub world_matrix: AffineTransform2D,
    pub anchor_point: Vec2,
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
}

impl EvaluatedTransform {
    /// Identity evaluated transform.
    pub const IDENTITY: Self = Self {
        local_matrix: AffineTransform2D::IDENTITY,
        world_matrix: AffineTransform2D::IDENTITY,
        anchor_point: Vec2::ZERO,
        position: Vec2::ZERO,
        scale: Vec2::SCALE_100,
        rotation: 0.0,
    };

    /// Construct an evaluated transform with explicit components and an optional parent world transform.
    pub fn from_components(
        position: Vec2,
        scale: Vec2,
        rotation: f32,
        anchor_point: Vec2,
        parent_world_matrix: Option<&AffineTransform2D>,
    ) -> Self {
        let local_matrix =
            AffineTransform2D::from_transform_components(position, scale, rotation, anchor_point);
        let world_matrix = match parent_world_matrix {
            Some(parent_world) => *parent_world * local_matrix,
            None => local_matrix,
        };

        Self {
            local_matrix,
            world_matrix,
            anchor_point,
            position,
            scale,
            rotation,
        }
    }

    /// Construct an evaluated transform from a standalone layer `project::Transform` with no parent at a specific timecode.
    pub fn from_node_transform_at(transform: &Transform, time: &TimeCode) -> Self {
        let (anchor, pos, scale, rot) = transform.evaluate_at(time);
        Self::from_components(pos, scale, rot, anchor, None)
    }

    /// Construct an evaluated transform from a standalone layer `project::Transform` with no parent using its current static values.
    pub fn from_node_transform(transform: &Transform) -> Self {
        let anchor = *transform.anchor_point.value();
        let pos = *transform.position.value();
        let scale = *transform.scale.value();
        let rot = *transform.rotation.value();
        Self::from_components(pos, scale, rot, anchor, None)
    }

    /// Map a point from layer local coordinates to world coordinates: `P_world = M_world * P_local`.
    pub fn local_to_world_point(&self, point: Vec2) -> Vec2 {
        self.world_matrix.transform_point(point)
    }

    /// Map a point from world coordinates to layer local coordinates: `P_local = M_world^-1 * P_world`.
    /// Returns `None` if the world transform is non-invertible.
    pub fn world_to_local_point(&self, point: Vec2) -> Option<Vec2> {
        self.world_matrix.transform_point_inverse(point)
    }

    /// Map an axis-aligned bounding box from layer local coordinates to world coordinates.
    pub fn local_to_world_bbox(&self, bbox: &BoundingBox2D) -> BoundingBox2D {
        bbox.transform(&self.world_matrix)
    }

    /// Map an axis-aligned bounding box from world coordinates to layer local coordinates.
    /// Returns `None` if the world transform is non-invertible.
    pub fn world_to_local_bbox(&self, bbox: &BoundingBox2D) -> Option<BoundingBox2D> {
        bbox.transform_inverse(&self.world_matrix)
    }

    /// Calculate the world-space bounding box given layer local dimensions (0, 0, width, height).
    pub fn world_bounds(&self, width: f32, height: f32) -> BoundingBox2D {
        let local_bbox = BoundingBox2D::from_origin_size(Vec2::ZERO, Vec2::new(width, height));
        self.local_to_world_bbox(&local_bbox)
    }

    /// Check if the evaluated world transform is invertible.
    pub fn is_invertible(&self) -> bool {
        self.world_matrix.is_invertible()
    }
}

impl Default for EvaluatedTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Resolver for hierarchical scene graph transforms.
///
/// Traverses the scene graph in topological evaluation order (`evaluation_order`),
/// ensuring that each parent layer's world transform is evaluated before any of its children.
#[derive(Debug, Clone, Default)]
pub struct TransformResolver;

impl TransformResolver {
    /// Resolve all layer transforms for a scene graph in topological evaluation order at a specific timecode.
    pub fn resolve_scene_graph_at(
        graph: &SceneGraph,
        time: &TimeCode,
    ) -> Result<HashMap<String, EvaluatedTransform>, SceneGraphError> {
        let eval_order = graph.evaluation_order()?;
        let mut resolved: HashMap<String, EvaluatedTransform> =
            HashMap::with_capacity(eval_order.len());

        for node in eval_order {
            let (anchor, position, scale, rotation) = node.transform.evaluate_at(time);

            let local_matrix =
                AffineTransform2D::from_transform_components(position, scale, rotation, anchor);

            let world_matrix = if let Some(ref parent_id) = node.parent_id {
                let parent_eval = resolved.get(parent_id).ok_or_else(|| {
                    SceneGraphError::ParentNotFound {
                        node_id: node.id.clone(),
                        parent_id: parent_id.clone(),
                    }
                })?;
                parent_eval.world_matrix * local_matrix
            } else {
                local_matrix
            };

            resolved.insert(
                node.id.clone(),
                EvaluatedTransform {
                    local_matrix,
                    world_matrix,
                    anchor_point: anchor,
                    position,
                    scale,
                    rotation,
                },
            );
        }

        Ok(resolved)
    }

    /// Resolve all layer transforms for a scene graph in topological evaluation order at zero timecode.
    pub fn resolve_scene_graph(
        graph: &SceneGraph,
    ) -> Result<HashMap<String, EvaluatedTransform>, SceneGraphError> {
        Self::resolve_scene_graph_at(graph, &TimeCode::zero(graph.frame_rate))
    }
}
