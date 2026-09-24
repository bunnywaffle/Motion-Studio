use project::{BlendMode, Color};

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
        self.blend_over_at(src, mode, 0, 0)
    }

    /// Blend-mode composite with destination pixel coords (used by the
    /// Dissolve dither so the speckle pattern is spatially stable).
    pub fn blend_over_at(&mut self, src: Px, mode: BlendMode, x: i32, y: i32) {
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
        if mode == BlendMode::Dissolve {
            // Dithered take: keep the source pixel (made opaque) with
            // probability sa, else leave the backdrop untouched. An opaque
            // speckle covers, so the composite is just the speckle.
            let h = ((x as u32).wrapping_mul(0x85eb_ca6b)
                ^ (y as u32).wrapping_mul(0xc2b2_ae35))
                % 1000;
            if h as f32 / 1000.0 > sa {
                return;
            }
            let ia = 1.0 / sa.max(1e-6);
            self.r = (src.r * ia).clamp(0.0, 1.0);
            self.g = (src.g * ia).clamp(0.0, 1.0);
            self.b = (src.b * ia).clamp(0.0, 1.0);
            self.a = 1.0;
            return;
        }
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

pub(crate) fn blend_color(mode: BlendMode, dst: [f32; 3], src: [f32; 3]) -> [f32; 3] {
    // Non-separable modes work on whole triplets (PDF SetLum/SetSat).
    match mode {
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => {
            return blend_color_nsep(mode, dst, src);
        }
        BlendMode::Normal => {
            return [src[0].clamp(0.0, 1.0), src[1].clamp(0.0, 1.0), src[2].clamp(0.0, 1.0)];
        }
        // Dissolve never reaches the color math (dithered in blend_over_at).
        BlendMode::Dissolve => {
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
            // W3C/PDF Soft Light (not the legacy Pegtop approximation).
            BlendMode::SoftLight => {
                if s <= 0.5 {
                    d - (1.0 - 2.0 * s) * d * (1.0 - d)
                } else if d <= 0.25 {
                    d + (2.0 * s - 1.0) * (((16.0 * d - 12.0) * d + 4.0) * d)
                } else {
                    d + (2.0 * s - 1.0) * (d.sqrt() - d)
                }
            }
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

/// Non-separable blend modes per the PDF reference (luminance-preserving
/// Hue/Saturation/Color/Luminosity via Lum/SetLum/SetSat — NOT naive HSL
/// channel swaps, which visibly diverge from Photoshop/AE).
fn nsep_lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn nsep_clip(c: [f32; 3]) -> [f32; 3] {
    let l = nsep_lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut r = c;
    if n < 0.0 {
        let denom = (l - n).max(1e-6);
        for v in r.iter_mut() {
            *v = l + ((*v - l) * l / denom);
        }
    }
    if x > 1.0 {
        let denom = (x - l).max(1e-6);
        for v in r.iter_mut() {
            *v = l + ((*v - l) * (1.0 - l) / denom);
        }
    }
    r
}

fn nsep_sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn nsep_set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - nsep_lum(c);
    nsep_clip([c[0] + d, c[1] + d, c[2] + d])
}

fn nsep_set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let (mn, mx) = (
        c[0].min(c[1]).min(c[2]),
        c[0].max(c[1]).max(c[2]),
    );
    if mx > mn {
        let mut r = [0.0; 3];
        for i in 0..3 {
            r[i] = if (c[i] - mx).abs() < 1e-6 {
                s
            } else if (c[i] - mn).abs() < 1e-6 {
                0.0
            } else {
                (c[i] - mn) * s / (mx - mn)
            };
        }
        r
    } else {
        [0.0, 0.0, 0.0]
    }
}

fn blend_color_nsep(mode: BlendMode, dst: [f32; 3], src: [f32; 3]) -> [f32; 3] {
    let r = match mode {
        BlendMode::Hue => nsep_set_lum(nsep_set_sat(src, nsep_sat(dst)), nsep_lum(dst)),
        BlendMode::Saturation => nsep_set_lum(nsep_set_sat(dst, nsep_sat(src)), nsep_lum(dst)),
        BlendMode::Color => nsep_set_lum(src, nsep_lum(dst)),
        BlendMode::Luminosity => nsep_set_lum(dst, nsep_lum(src)),
        _ => src,
    };
    [r[0].clamp(0.0, 1.0), r[1].clamp(0.0, 1.0), r[2].clamp(0.0, 1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4 && (a[2] - b[2]).abs() < 1e-4
    }

    #[test]
    fn separable_modes_match_reference() {
        // Photoshop reference values (sRGB, straight).
        assert!(close(blend_color(BlendMode::Multiply, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]), [0.25, 0.25, 0.25]));
        assert!(close(blend_color(BlendMode::Screen, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]), [0.75, 0.75, 0.75]));
        assert!(close(blend_color(BlendMode::Overlay, [0.25, 0.25, 0.25], [0.5, 0.5, 0.5]), [0.25, 0.25, 0.25]));
        assert!(close(blend_color(BlendMode::Overlay, [0.75, 0.75, 0.75], [0.5, 0.5, 0.5]), [0.75, 0.75, 0.75]));
        assert!(close(blend_color(BlendMode::HardLight, [0.5, 0.5, 0.5], [0.25, 0.25, 0.25]), [0.25, 0.25, 0.25]));
        assert!(close(blend_color(BlendMode::Difference, [0.8, 0.2, 0.1], [0.3, 0.9, 0.4]), [0.5, 0.7, 0.3]));
        assert!(close(blend_color(BlendMode::Exclusion, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]), [0.5, 0.5, 0.5]));
        assert!(close(blend_color(BlendMode::ColorDodge, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]), [1.0, 1.0, 1.0]));
        assert!(close(blend_color(BlendMode::ColorBurn, [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]), [0.0, 0.0, 0.0]));
        assert!(close(blend_color(BlendMode::Darken, [0.2, 0.8, 0.5], [0.7, 0.3, 0.5]), [0.2, 0.3, 0.5]));
        assert!(close(blend_color(BlendMode::Lighten, [0.2, 0.8, 0.5], [0.7, 0.3, 0.5]), [0.7, 0.8, 0.5]));
    }

    #[test]
    fn soft_light_matches_spec() {
        // 50% source is identity.
        assert!(close(blend_color(BlendMode::SoftLight, [0.3, 0.3, 0.3], [0.5, 0.5, 0.5]), [0.3, 0.3, 0.3]));
        // White source on dark dst (D<=0.25 branch):
        // 0.25 + (((16*.25-12)*.25+4)*.25) = 0.25 + 0.5 = 0.75.
        assert!(close(blend_color(BlendMode::SoftLight, [0.25, 0.25, 0.25], [1.0, 1.0, 1.0]), [0.75, 0.75, 0.75]));
        // Black source on dark dst: 0.25 - 0.25*0.75 = 0.0625.
        assert!(close(blend_color(BlendMode::SoftLight, [0.25, 0.25, 0.25], [0.0, 0.0, 0.0]), [0.0625, 0.0625, 0.0625]));
        // White source lifts mids toward sqrt: 0.5 + (0.7071-0.5) = 0.7071.
        let r = blend_color(BlendMode::SoftLight, [0.5, 0.5, 0.5], [1.0, 1.0, 1.0]);
        assert!((r[0] - 0.7071).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn nonseparable_modes_preserve_spec_invariants() {
        let dst = [0.2, 0.2, 0.8];
        let src = [0.9, 0.1, 0.1];
        // Hue keeps backdrop luminance.
        let h = blend_color(BlendMode::Hue, dst, src);
        assert!((nsep_lum(h) - nsep_lum(dst)).abs() < 1e-4, "{h:?}");
        // Color keeps backdrop luminance too.
        let c = blend_color(BlendMode::Color, dst, src);
        assert!((nsep_lum(c) - nsep_lum(dst)).abs() < 1e-4, "{c:?}");
        // Luminosity takes source luminance, keeps backdrop hue/sat shape.
        let l = blend_color(BlendMode::Luminosity, dst, src);
        assert!((nsep_lum(l) - nsep_lum(src)).abs() < 1e-4, "{l:?}");
        // Achromatic source in Color mode yields neutral gray at dst lum.
        let g = blend_color(BlendMode::Color, [0.4, 0.5, 0.6], [0.7, 0.7, 0.7]);
        assert!((g[0] - g[1]).abs() < 1e-4 && (g[1] - g[2]).abs() < 1e-4, "{g:?}");
        // Saturation with gray source drains chroma.
        let s = blend_color(BlendMode::Saturation, [0.8, 0.2, 0.2], [0.5, 0.5, 0.5]);
        assert!((s[0] - s[1]).abs() < 1e-4 && (s[1] - s[2]).abs() < 1e-4, "{s:?}");
    }

    #[test]
    fn dissolve_dithers_by_alpha() {
        // Fully transparent source never touches dst, anywhere.
        for (x, y) in [(0, 0), (7, 3), (100, 200)] {
            let mut d = Px { r: 0.2, g: 0.3, b: 0.4, a: 0.9 };
            d.blend_over_at(Px { r: 1.0, g: 0.0, b: 0.0, a: 0.0 }, BlendMode::Dissolve, x, y);
            assert_eq!((d.r, d.g, d.b, d.a), (0.2, 0.3, 0.4, 0.9));
        }
        // Opaque source always covers with its straight color.
        let mut d = Px::clear();
        d.blend_over_at(Px { r: 0.5, g: 0.25, b: 0.1, a: 1.0 }, BlendMode::Dissolve, 3, 7);
        assert!(close([d.r, d.g, d.b], [0.5, 0.25, 0.1]) && (d.a - 1.0).abs() < 1e-6);
        // Half alpha keeps ~half the pixels (deterministic pattern).
        let mut kept = 0;
        for y in 0..40 {
            for x in 0..40 {
                let mut d = Px::clear();
                d.blend_over_at(Px { r: 1.0, g: 1.0, b: 1.0, a: 0.5 }, BlendMode::Dissolve, x, y);
                if d.a > 0.5 {
                    kept += 1;
                }
            }
        }
        let ratio = kept as f32 / 1600.0;
        assert!((ratio - 0.5).abs() < 0.05, "{ratio}");
    }
}

// ---------------------------------------------------------------------------
