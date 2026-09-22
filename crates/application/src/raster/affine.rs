
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
