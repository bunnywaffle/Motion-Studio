use super::pixel::Px;
use project::Color;

// Buffers
// ---------------------------------------------------------------------------

pub struct FloatBuf {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Px>,
}

impl FloatBuf {
    pub fn clear(w: u32, h: u32) -> Self {
        Self { w, h, px: vec![Px::clear(); (w.max(1) * h.max(1)) as usize] }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Px {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return Px::clear();
        }
        self.px[(y as u32 * self.w + x as u32) as usize]
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, p: Px) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        self.px[(y as u32 * self.w + x as u32) as usize] = p;
    }

    /// Bilinear sample in pixel space (straight alpha).
    pub fn sample(&self, x: f32, y: f32) -> Px {
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fx = (x - x0 as f32).clamp(0.0, 1.0);
        let fy = (y - y0 as f32).clamp(0.0, 1.0);
        let a = self.get(x0, y0);
        let b = self.get(x0 + 1, y0);
        let c = self.get(x0, y0 + 1);
        let d = self.get(x0 + 1, y0 + 1);
        let mix = |p: f32, q: f32, r: f32, s: f32| {
            p * (1.0 - fx) * (1.0 - fy) + q * fx * (1.0 - fy) + r * (1.0 - fx) * fy + s * fx * fy
        };
        Px {
            r: mix(a.r, b.r, c.r, d.r),
            g: mix(a.g, b.g, c.g, d.g),
            b: mix(a.b, b.b, c.b, d.b),
            a: mix(a.a, b.a, c.a, d.a),
        }
    }

    pub fn average(&self) -> Color {
        if self.px.is_empty() {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0, 0.0, 0.0);
        for p in &self.px {
            r += p.r as f64;
            g += p.g as f64;
            b += p.b as f64;
            a += p.a as f64;
        }
        let n = self.px.len() as f64;
        let avg_a = (a / n) as f32;
        if avg_a <= 1e-6 {
            return Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        // Un-premultiply the mean for a straight average color.
        Color::rgba(
            ((r / n) as f32 / avg_a).clamp(0.0, 1.0),
            ((g / n) as f32 / avg_a).clamp(0.0, 1.0),
            ((b / n) as f32 / avg_a).clamp(0.0, 1.0),
            avg_a.clamp(0.0, 1.0),
        )
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let (r, g, b) = if p.a <= 1e-6 {
                (0.0, 0.0, 0.0)
            } else {
                (p.r / p.a, p.g / p.a, p.b / p.a)
            };
            out.push((r.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((g.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((b.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((p.a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }

    /// Straight-alpha BGRA8 bytes for direct `RenderImage` upload (GPUI
    /// caches BGRA frames; feeding PNGs would re-encode, re-hash, and
    /// re-decode per layer per tick with async pop-in).
    pub fn to_bgra8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let (r, g, b) = if p.a <= 1e-6 {
                (0.0, 0.0, 0.0)
            } else {
                (p.r / p.a, p.g / p.a, p.b / p.a)
            };
            out.push((b.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((g.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((r.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push((p.a.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }
}

/// Gaussian blur a straight-alpha buffer in place (delegates to the
/// tested renderer CPU kernel on packed bytes). Radius is capped for
/// preview speed; export uses the full value.
pub fn blur_buffer(buf: &mut FloatBuf, radius_px: f32) {
    let radius_px = radius_px.clamp(0.0, 48.0);
    if radius_px < 0.5 || buf.w == 0 || buf.h == 0 {
        return;
    }
    let mut bytes = buf.to_rgba8();
    renderer::gaussian_blur_rgba(&mut bytes, buf.w, buf.h, radius_px);
    for (i, p) in buf.px.iter_mut().enumerate() {
        let a = bytes[i * 4 + 3] as f32 / 255.0;
        *p = Px {
            r: bytes[i * 4] as f32 / 255.0 * a,
            g: bytes[i * 4 + 1] as f32 / 255.0 * a,
            b: bytes[i * 4 + 2] as f32 / 255.0 * a,
            a,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_softens_hard_edge() {
        let mut buf = FloatBuf::clear(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                if x >= 16 {
                    buf.px[(y * 32 + x) as usize] = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
                }
            }
        }
        blur_buffer(&mut buf, 4.0);
        // Edge column bleeds both ways.
        assert!(buf.get(15, 16).r > 0.05 && buf.get(15, 16).r < 0.95);
        assert!(buf.get(16, 16).r > 0.05 && buf.get(16, 16).r < 1.0);
        // Zero radius is a no-op (and huge radii stay bounded/fast).
        let mut buf2 = FloatBuf::clear(8, 8);
        blur_buffer(&mut buf2, 0.0);
        blur_buffer(&mut buf2, 500.0);
        assert!(buf2.px.iter().all(|p| p.a == 0.0));
    }
}
