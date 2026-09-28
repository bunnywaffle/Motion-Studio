//! CPU raster viewport compositor: true-color per-pixel previews.
//!
//! gpui renders SVG only as monochrome alpha masks and offers no Div
//! rotation, so rotated or spatially-filtered content cannot preview
//! through divs. This module rasterizes each layer to an AABB-sized RGBA
//! pixmap at canvas scale instead:
//!
//! - base content: solids, shapes (rect/ellipse/pen polyline), cosmic-text
//!   glyphs (weight, italic, tracking, leading, align, caps, stroke,
//!   baseline, box wrap, bevel emboss), decoded images, video slate;
//! - per-pixel effects with real uv/position: checkerboard, gradient,
//!   tiler, warp, perspective, noise, blur, bloom, drop shadow, plus the
//!   whole non-spatial stack through `process_color` (tint, exposure,
//!   vibrance, ...);
//! - Shader Lab runs per-pixel through the CPU interpreter when paused,
//!   and falls back to a cheap probe wash during playback;
//! - one affine rotate/scale/translate blit with bilinear sampling,
//!   opacity, blend modes, then PNG encode for `gpui::img`.
//!
//! Adjustment layers post-process the composite beneath them, matching
//! the fold-in semantics used elsewhere.

pub mod affine;
pub mod buffer;
pub mod comp;
pub mod effects;
pub mod layer;
pub mod mask;
pub mod pixel;
pub mod shapes;
pub mod stock;
pub mod text;

pub use affine::{Aff, aff_apply, aff_invert, aff_mul, fold_transform, skew_about};
pub use buffer::{FloatBuf, blur_buffer};
pub use comp::rasterize_comp;
pub use effects::{RasterFx, apply_effect_pixels};
pub use layer::{RasterEntry, decoded_asset, gizmo_local_corners, layer_cache_key, layer_local_box, rasterize_layer};
pub use mask::apply_masks;
pub use pixel::Px;
pub use stock::apply_stock;
pub use text::{TextSpec, raster_text};

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::EvaluatedEffectType;
    use project::{BlendMode, Color, TextAlign};
    use crate::raster::{
        pixel::blend_color,
        shapes::{fill_ellipse, fill_rect, fill_rect_gradient},
    };

    #[test]
    fn affine_invert_round_trip() {        let m = Aff { a: 2.0, b: 0.5, c: -1.0, d: 3.0, tx: 10.0, ty: -4.0 };
        let inv = aff_invert(m).unwrap();
        let (x, y) = (7.0, -3.0);
        let (wx, wy) = aff_apply(m, x, y);
        let (rx, ry) = aff_apply(inv, wx, wy);
        assert!((rx - x).abs() < 1e-4 && (ry - y).abs() < 1e-4);
        // Singular matrix has no inverse.
        assert!(aff_invert(Aff { a: 0.0, b: 0.0, c: 0.0, d: 0.0, tx: 0.0, ty: 0.0 }).is_none());
    }

    #[test]
    fn affine_mul_and_skew() {
        let id = Aff { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };
        let m = Aff { a: 2.0, b: 0.0, c: 1.0, d: 3.0, tx: 5.0, ty: -2.0 };
        let r = aff_mul(id, m);
        assert!((r.a - 2.0).abs() < 1e-5 && (r.tx - 5.0).abs() < 1e-5);
        let r2 = aff_mul(m, id);
        assert!((r2.d - 3.0).abs() < 1e-5 && (r2.ty + 2.0).abs() < 1e-5);
        // Zero skew is identity; 45-degree skew shears x by y.
        let s0 = skew_about(0.0, 0.0, 10.0, 10.0);
        let (zx, zy) = aff_apply(s0, 4.0, 7.0);
        assert!((zx - 4.0).abs() < 1e-4 && (zy - 7.0).abs() < 1e-4);
        let s45 = skew_about(45.0, 0.0, 0.0, 0.0);
        let (kx, ky) = aff_apply(s45, 3.0, 2.0);
        assert!((kx - 5.0).abs() < 1e-4 && (ky - 2.0).abs() < 1e-4);
        // Pivot fixed point: center maps to itself.
        let (px, py) = aff_apply(skew_about(20.0, -10.0, 50.0, 40.0), 50.0, 40.0);
        assert!((px - 50.0).abs() < 1e-3 && (py - 40.0).abs() < 1e-3);
    }

    #[test]
    fn blend_modes_sanity() {
        let white = [1.0, 1.0, 1.0];
        let black = [0.0, 0.0, 0.0];
        let half = [0.5, 0.5, 0.5];
        assert_eq!(blend_color(BlendMode::Multiply, white, half), [0.5, 0.5, 0.5]);
        assert_eq!(blend_color(BlendMode::Screen, black, half), [0.5, 0.5, 0.5]);
        assert_eq!(blend_color(BlendMode::Add, half, half), [1.0, 1.0, 1.0]);
        let d = blend_color(BlendMode::Overlay, [0.2, 0.2, 0.2], [0.8, 0.8, 0.8]);
        assert!(d[0] > 0.2 && d[0] < 1.0);
    }

    #[test]
    fn rect_and_ellipse_fill() {
        let mut buf = FloatBuf::clear(20, 20);
        fill_rect(&mut buf, 20.0, 20.0, 0.0, Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
        assert!(buf.get(10, 10).r > 0.9);
        let red = buf.px.iter().filter(|p| p.r > 0.9 && p.a > 0.9).count();
        assert!(red > 300, "red {red}");
        let mut buf = FloatBuf::clear(21, 21);
        fill_ellipse(&mut buf, 10.0, 10.0, Px { r: 0.0, g: 1.0, b: 0.0, a: 1.0 });
        assert!(buf.get(10, 10).g > 0.9);
        assert!(buf.get(0, 0).a < 0.5);
    }

    #[test]
    fn text_rasterizes_with_sane_ink() {
        let spec = TextSpec {
            text: "Ag",
            family: "Arial",
            size: 64.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            fill_gradient: None,
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 0.0,
            align: TextAlign::Left,
            all_caps: false,
            stroke_w: 0.0,
            stroke_col: Px::clear(),
            stroke_gradient: None,
            baseline_shift: 0.0,
            box_w: 0.0,
            bevel: None,
        };
        let (buf, (x0, y0, x1, y1)) = raster_text(&spec);
        assert!(x1 > x0 && y1 > y0, "text must leave ink");
        let (iw, ih) = (x1 - x0, y1 - y0);
        // Plausible glyph extents for 64px caps text.
        assert!(iw > 30.0 && iw < 300.0, "ink width {iw}");
        assert!(ih > 25.0 && ih < 110.0, "ink height {ih}");
        // Ink sits in the upper portion (baseline layout, not sunk).
        assert!(y0 < buf.h as f32 * 0.6, "ink top {y0} of {}", buf.h);
        assert!(x0 < buf.w as f32 * 0.4, "ink left {x0}");
        let _ = (x0, y0, x1, y1);
    }

    #[test]
    fn text_outline_touches_fill_without_moat() {
        // "H" at 48px with a wide red outline over a white fill: every
        // pixel within 2px of fill ink must be covered (the outline must
        // touch the fill instead of floating detached with a clear moat).
        let spec = TextSpec {
            text: "H",
            family: "Arial",
            size: 48.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            fill_gradient: None,
            weight: 700,
            italic: false,
            tracking: 0.0,
            leading: 0.0,
            align: TextAlign::Left,
            all_caps: false,
            stroke_w: 10.0,
            stroke_col: Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
            stroke_gradient: None,
            baseline_shift: 0.0,
            box_w: 0.0,
            bevel: None,
        };
        let (buf, _) = raster_text(&spec);
        let (w, h) = (buf.w as usize, buf.h as usize);
        let at = |x: isize, y: isize| -> f32 {
            if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                0.0
            } else {
                buf.px[y as usize * w + x as usize].a
            }
        };
        let is_fill = |x: isize, y: isize| -> bool {
            if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                return false;
            }
            let p = buf.px[y as usize * w + x as usize];
            p.a > 0.5 && p.g > 0.5
        };
        // Dilate the fill mask by 2px (3x3 max, twice).
        let mut band = vec![false; w * h];
        for y in 0..h as isize {
            for x in 0..w as isize {
                'outer: for r in 1..=2 {
                    for dy in -(r)..=r {
                        for dx in -(r)..=r {
                            if is_fill(x + dx, y + dy) {
                                band[y as usize * w + x as usize] = true;
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
        // Outline pixels are red-dominant; count them (must exist).
        let mut outline = 0usize;
        let mut moat = 0usize;
        for y in 0..h as isize {
            for x in 0..w as isize {
                let p = buf.px[y as usize * w + x as usize];
                let is_outline = p.a > 0.1 && p.r > 0.5 && p.g < 0.5;
                if is_outline {
                    outline += 1;
                }
                // Band pixel that is neither fill nor covered = moat hole.
                if band[y as usize * w + x as usize] && !is_fill(x, y) && at(x, y) < 0.1 {
                    moat += 1;
                }
            }
        }
        assert!(outline > 50, "wide outline must leave pixels, got {outline}");
        assert_eq!(moat, 0, "outline must touch the fill (no transparent moat)");
    }

    #[test]
    fn text_outline_thickness_scales_with_width() {
        // Same glyph at stroke 2 vs 10: the dilated ring area must grow
        // substantially (a stamped ring keeps near-constant area).
        fn outline_count(stroke_w: f32) -> usize {
            let spec = TextSpec {
                text: "O",
                family: "Arial",
                size: 64.0,
                fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                fill_gradient: None,
                weight: 400,
                italic: false,
                tracking: 0.0,
                leading: 0.0,
                align: TextAlign::Left,
                all_caps: false,
                stroke_w,
                stroke_col: Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
                stroke_gradient: None,
                baseline_shift: 0.0,
                box_w: 0.0,
                bevel: None,
            };
            let (buf, _) = raster_text(&spec);
            buf.px
                .iter()
                .filter(|p| p.a > 0.1 && p.r > 0.5 && p.g < 0.5)
                .count()
        }
        let thin = outline_count(2.0);
        let thick = outline_count(10.0);
        assert!(thin > 20, "thin outline exists, got {thin}");
        assert!(
            thick as f32 > thin as f32 * 2.0,
            "outline must grow with width: thin={thin} thick={thick}"
        );
    }

    #[test]
    fn checker_and_gradient_cover() {
        let mut buf = FloatBuf::clear(32, 32);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let fx = EvaluatedEffectType::Checkerboard {
            size: 8.0,
            color_a: Color::BLACK,
            color_b: Color::WHITE,
        };
        let ctx = RasterFx { time_s: 0.0, frame: 0, res_w: 32.0, res_h: 32.0, duration_s: 0.0, playing: false };
        apply_effect_pixels(&mut buf, 32.0, 32.0, &fx, &ctx);
        let dark = buf.px.iter().filter(|p| p.r < 0.1).count();
        let light = buf.px.iter().filter(|p| p.r > 0.9).count();
        assert!(dark > 100 && light > 100, "{dark} {light}");
        let mut buf = FloatBuf::clear(32, 1);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let fx = EvaluatedEffectType::GradientRamp {
            gradient: project::FillGradient::two_color(Color::BLACK, Color::WHITE, 0.0),
        };
        apply_effect_pixels(&mut buf, 32.0, 1.0, &fx, &ctx);
        assert!(buf.px[0].r < 0.1);
        assert!(buf.px[31].r > 0.9);
    }

    #[test]
    fn gradient_ramp_three_stops_hit_middle_color() {
        let mut buf = FloatBuf::clear(32, 1);
        for p in buf.px.iter_mut() {
            *p = Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        }
        let fx = EvaluatedEffectType::GradientRamp {
            gradient: project::FillGradient {
                stops: vec![
                    project::GradientStop::new(0.0, Color::BLACK),
                    project::GradientStop::new(0.5, Color::RED),
                    project::GradientStop::new(1.0, Color::WHITE),
                ],
                angle: 0.0,
            },
        };
        let ctx = RasterFx { time_s: 0.0, frame: 0, res_w: 32.0, res_h: 1.0, duration_s: 0.0, playing: false };
        apply_effect_pixels(&mut buf, 32.0, 1.0, &fx, &ctx);
        assert!(buf.px[0].r < 0.1);
        let mid = buf.px[16];
        assert!(mid.r > 0.7 && mid.g < 0.3, "{mid:?}");
        assert!(buf.px[31].r > 0.9);
    }

    #[test]
    fn rect_gradient_fill_varies_along_axis() {
        let mut buf = FloatBuf::clear(32, 32);
        let grad = project::FillGradient::two_color(Color::BLACK, Color::WHITE, 0.0);
        fill_rect_gradient(&mut buf, 32.0, 32.0, 0.0, &grad);
        // Middle row, inset from the 1px edge AA: black -> mid -> white.
        let (l, m, r) = (buf.px[16 * 32 + 2], buf.px[16 * 32 + 16], buf.px[16 * 32 + 29]);
        assert!(l.r < 0.1 && (l.a - 1.0).abs() < 1e-5, "{l:?}");
        assert!((m.r - 0.5).abs() < 0.1, "{m:?}");
        assert!(r.r > 0.9, "{r:?}");
    }

    #[test]
    fn text_on_path_places_ink_along_curve() {
        use crate::raster::text::{TextSpec, raster_text_on_path};
        let spec = TextSpec {
            text: "Hi",
            family: "Arial",
            size: 48.0,
            fill: Px { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            fill_gradient: None,
            weight: 400,
            italic: false,
            tracking: 0.0,
            leading: 0.0,
            align: TextAlign::Center,
            all_caps: false,
            stroke_w: 0.0,
            stroke_col: Px::clear(),
            stroke_gradient: None,
            baseline_shift: 0.0,
            box_w: 0.0,
            bevel: None,
        };
        // Horizontal baseline: ink sits near y=0, spread along x.
        let mut straight = project::Path::new();
        straight.line_to(project::Vec2::new(-100.0, 0.0));
        straight.line_to(project::Vec2::new(100.0, 0.0));
        let (buf, ink, _origin) = raster_text_on_path(&spec, &straight);
        assert!(ink.2 > ink.0 + 20.0, "text spreads horizontally: {ink:?}");
        // Ink y-range stays within a line height of the baseline.
        assert!((ink.3 - ink.1).abs() < 80.0, "{ink:?}");
        let inked: usize = buf.px.iter().filter(|p| p.a > 0.05).count();
        assert!(inked > 50, "glyphs rasterized: {inked}");
    }
}
