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

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.0;
    if (mx - mn).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if (mx - r).abs() < 1e-6 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (mx - g).abs() < 1e-6 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h.fract(), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s.abs() < 1e-6 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hk = h.fract();
    let tc = |t: f32| {
        let t = t.fract();
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [tc(hk + 1.0 / 3.0), tc(hk), tc(hk - 1.0 / 3.0)]
}

pub(crate) fn blend_color(mode: BlendMode, dst: [f32; 3], src: [f32; 3]) -> [f32; 3] {
    // HSL-space modes work on whole triplets.
    match mode {
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => {
            return blend_color_hsl(mode, dst, src);
        }
        BlendMode::Normal | BlendMode::Dissolve => {
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
            BlendMode::SoftLight => (1.0 - 2.0 * s) * d * d + 2.0 * s * d,
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

fn blend_color_hsl(mode: BlendMode, dst: [f32; 3], src: [f32; 3]) -> [f32; 3] {
    let (hd, sd, ld) = rgb_to_hsl(dst[0], dst[1], dst[2]);
    let (hs, ss, ls) = rgb_to_hsl(src[0], src[1], src[2]);
    let r = match mode {
        BlendMode::Hue => hsl_to_rgb(hs, sd, ld),
        BlendMode::Saturation => hsl_to_rgb(hd, ss, ld),
        BlendMode::Color => hsl_to_rgb(hs, ss, ld),
        BlendMode::Luminosity => hsl_to_rgb(hd, sd, ls),
        _ => src,
    };
    [r[0].clamp(0.0, 1.0), r[1].clamp(0.0, 1.0), r[2].clamp(0.0, 1.0)]
}

// ---------------------------------------------------------------------------
