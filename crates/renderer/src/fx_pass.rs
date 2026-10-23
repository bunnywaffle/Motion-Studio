//! Native wgpu effect passes: run any WGSL filter on the GPU.
//!
//! [`FxPass`] is a generic fullscreen-triangle pass: source texture +
//! sampler + one uniform block. Each stock effect maps to a WGSL body plus
//! packed uniforms via [`fx_source_for`] / [`pack_stock_uniforms`], so the
//! whole catalogue runs natively on wgpu (export path) with the same math
//! the CPU preview uses. Multi-tap spatial effects (blurs, morphology,
//! unsharp) run as multi-pass or CPU convolution â€” see each kernel's docs.

use crate::device::{GpuContext, RenderTarget};
use bytemuck::{Pod, Zeroable};
use project::StockPlugin;

/// Uniform block shared by every stock WGSL twin: 16 param floats, 4 RGBA
/// color slots (stock `colors`, descriptor defaults when the project
/// predates the slot), then `(time_s, width_px, height_px, seed)`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct FxUniforms {
    pub params: [[f32; 4]; 4],
    pub colors: [[f32; 4]; 4],
    pub misc: [f32; 4],
}

impl FxUniforms {
    /// Pack evaluated scalar params + color slots + frame context.
    pub fn pack(params: &[f32], colors: &[[f32; 4]], time_s: f32, w: f32, h: f32, seed: f32) -> Self {
        let mut p = [[0.0f32; 4]; 4];
        for (i, v) in params.iter().take(16).enumerate() {
            p[i / 4][i % 4] = *v;
        }
        let mut c = [[0.0f32; 4]; 4];
        for (i, v) in colors.iter().take(4).enumerate() {
            c[i] = *v;
        }
        Self { params: p, colors: c, misc: [time_s, w, h, seed] }
    }
}

/// Fullscreen-triangle vertex stage shared by all effect passes.
pub const FX_VERT: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
    out.uv = vec2<f32>(positions[vi].x * 0.5 + 0.5, 0.5 - positions[vi].y * 0.5);
    return out;
}
"#;

/// Bindings shared by every pass: texture(0), sampler(1), uniforms(2).
pub const FX_BINDINGS_WGSL: &str = r#"
struct FxUniforms {
    params: array<vec4<f32>, 4>,
    colors: array<vec4<f32>, 4>,
    misc: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: FxUniforms;
"#;

/// Build a complete fragment-stage source for one effect body: samples the
/// source texture, runs `main_call` (which must assign `color`), and
/// returns it. `uv_calls` run first for pre-sample remaps.
/// Content hash for a graph (pass-cache key; `# ponytail:` Debug-format —
/// graphs are tiny, collisions astronomically unlikely, zero plumbing).
pub fn graph_key_of(graph: &project::ModifierGraph) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    graph.hash(&mut h);
    h.finish()
}

/// 8-tap ring average of the source around `uv` (Glow halo).
pub const FX_RING_WGSL: &str = r#"fn fx_ring(uv: vec2<f32>, r: f32) -> vec4<f32> {
    var s = vec4<f32>(0.0);
    s = s + textureSample(src_tex, src_sampler, uv + vec2<f32>(r, 0.0));
    s = s + textureSample(src_tex, src_sampler, uv - vec2<f32>(r, 0.0));
    s = s + textureSample(src_tex, src_sampler, uv + vec2<f32>(0.0, r));
    s = s + textureSample(src_tex, src_sampler, uv - vec2<f32>(0.0, r));
    let d = r * 0.7071;
    s = s + textureSample(src_tex, src_sampler, uv + vec2<f32>(d, d));
    s = s + textureSample(src_tex, src_sampler, uv + vec2<f32>(-d, d));
    s = s + textureSample(src_tex, src_sampler, uv + vec2<f32>(d, -d));
    s = s + textureSample(src_tex, src_sampler, uv - vec2<f32>(d, d));
    return s * 0.125;
}
"#;

/// 8-tap ring of the bright pass (Bloom halo): taps gated at `th`.
pub const FX_BRIGHT_RING_WGSL: &str = r#"fn fx_bright_ring(uv: vec2<f32>, r: f32, th: f32) -> vec4<f32> {
    var s = vec4<f32>(0.0);
    let t0 = vec4<f32>(th);
    let d = r * 0.7071;
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv + vec2<f32>(r, 0.0)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv - vec2<f32>(r, 0.0)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv + vec2<f32>(0.0, r)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv - vec2<f32>(0.0, r)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv + vec2<f32>(d, d)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv + vec2<f32>(-d, d)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv + vec2<f32>(d, -d)) - t0);
    s = s + max(vec4<f32>(0.0), textureSample(src_tex, src_sampler, uv - vec2<f32>(d, d)) - t0);
    return s * 0.125;
}
"#;

pub fn fx_fragment_src(effect_body: &str, uv_calls: &[&str], main_call: &str) -> String {    let mut src = String::from(FX_BINDINGS_WGSL);
    src.push_str(effect_body);
    src.push_str(
        r#"
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var uv = in.uv;
"#,
    );
    for call in uv_calls {
        src.push_str("    uv = ");
        src.push_str(call);
        src.push_str(";\n");
    }
    src.push_str("    var color = textureSample(src_tex, src_sampler, uv);\n    color = ");
    src.push_str(main_call);
    src.push_str(";\n    return color;\n}\n");
    src
}

/// Generic single-pass effect runner. The WGSL source must define
/// `vs_main`/`fs_main` with the `FxUniforms` block above (see
/// [`fx_fragment_src`]).
pub struct FxPass {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Packed float params for graph passes (empty for stock/builtin).
    pub graph_params: Vec<f32>,
}

impl FxPass {
    /// Compile a pass. Returns `Err` with the naga message on bad WGSL.
    pub fn new(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
        label: &str,
        wgsl: &str,
    ) -> Result<Self, String> {
        naga::front::wgsl::parse_str(wgsl)
            .map_err(|e| format!("fx pass {label} WGSL invalid: {e:?}"))?;
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let bind_group_layout =
            gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout =
            gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Ok(Self { pipeline, bind_group_layout, sampler, graph_params: Vec::new() })
    }

    /// Compile a pass for a supported stock plugin.
    pub fn for_stock(
        gpu: &GpuContext,
        plugin: StockPlugin,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        use crate::effect_filters as ef;
        let res = "vec2<f32>(u.misc.y, u.misc.z)";
        let (body, call, resample): (&str, &str, bool) = match plugin {
            StockPlugin::Posterize => (
                ef::stock_posterize().wgsl,
                "fx_stock_posterize(uv, color, u.params[0].x)",
                false,
            ),
            StockPlugin::Threshold => (
                ef::stock_threshold().wgsl,
                "fx_stock_threshold(uv, color, u.params[0].x, u.params[0].y)",
                false,
            ),
            StockPlugin::FilmGrain => (
                ef::stock_film_grain().wgsl,
                "fx_stock_film_grain(uv, color, u.params[0].x, u.misc.x, u.params[0].y)",
                false,
            ),
            StockPlugin::Scanlines => (
                ef::stock_scanlines().wgsl,
                "fx_stock_scanlines(uv, color, u.params[0].x, u.params[0].y, u.misc.z)",
                false,
            ),
            StockPlugin::Crop => (
                ef::stock_crop().wgsl,
                "fx_stock_crop(uv, color, u.params[0], vec2<f32>(u.misc.y, u.misc.z))",
                false,
            ),
            StockPlugin::TemperatureTint => (
                ef::stock_temperature_tint().wgsl,
                "fx_stock_temperature_tint(uv, color, u.params[0].xy)",
                false,
            ),
            StockPlugin::SpillSuppress => (
                ef::stock_spill_suppress().wgsl,
                "fx_stock_spill_suppress(uv, color, u.params[0].x)",
                false,
            ),
            StockPlugin::DifferenceKey => (
                ef::stock_difference_key().wgsl,
                "fx_stock_difference_key(uv, color, u.params[0].x, u.params[0].y, u.params[0].z)",
                false,
            ),
            StockPlugin::Wave => (
                ef::stock_wave_resample().wgsl,
                "fx_stock_wave_c(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                true,
            ),
            StockPlugin::Ripple => (
                ef::stock_ripple_resample().wgsl,
                "fx_stock_ripple_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Twirl => (
                ef::stock_twirl_resample().wgsl,
                "fx_stock_twirl_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Bulge => (
                ef::stock_bulge_resample().wgsl,
                "fx_stock_bulge_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Spherize => (
                ef::stock_spherize_resample().wgsl,
                "fx_stock_spherize_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::LensDistortion => (
                ef::stock_lens_distortion_resample().wgsl,
                "fx_stock_lens_distortion_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Mirror => (
                ef::stock_mirror_resample().wgsl,
                "fx_stock_mirror_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Repeat => (
                ef::stock_repeat_resample().wgsl,
                "fx_stock_repeat_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                false,
            ),
            StockPlugin::Offset => (
                ef::stock_offset_resample().wgsl,
                "fx_stock_offset_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Pixelate => (
                ef::stock_pixelate_resample().wgsl,
                "fx_stock_pixelate_c(uv, color, u.params[0].x, RES)",
                true,
            ),
            StockPlugin::Mosaic => (
                ef::stock_mosaic_resample().wgsl,
                "fx_stock_mosaic_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::ChromaticAberration => (
                ef::stock_chromatic_resample().wgsl,
                "fx_stock_chromatic_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::RgbSplit => (
                ef::stock_rgb_split_resample().wgsl,
                "fx_stock_rgb_split_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Emboss => (
                ef::stock_emboss_resample().wgsl,
                "fx_stock_emboss_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::EdgeDetect => (
                ef::stock_edge_resample().wgsl,
                "fx_stock_edge_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Cartoon => (
                ef::stock_cartoon_resample().wgsl,
                "fx_stock_cartoon_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Halftone => (
                ef::stock_halftone_resample().wgsl,
                "fx_stock_halftone_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::Card3d => (
                ef::stock_card_3d_resample().wgsl,
                "fx_stock_card_3d_c(uv, color, u.params[0].xyz, u.params[1].xyz, u.params[2].xyz, RES)",
                true,
            ),
            StockPlugin::CornerPin => (
                ef::stock_corner_pin_resample().wgsl,
                "fx_stock_corner_pin_c(uv, color, u.params[0].xyz, u.params[1].xyz, u.params[2].xyz, RES)",
                true,
            ),
            StockPlugin::Reframe => (
                ef::stock_reframe_resample().wgsl,
                "fx_stock_reframe_c(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                true,
            ),
            StockPlugin::Liquify => (
                ef::stock_liquify_resample().wgsl,
                "fx_stock_liquify_c(uv, color, u.params[0].xy, u.params[0].z, u.params[0].w, u.params[1].x, RES)",
                true,
            ),
            StockPlugin::Halo => (
                ef::stock_halo().wgsl,
                "fx_stock_halo(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                false,
            ),
            StockPlugin::Streaks => (
                ef::stock_streaks().wgsl,
                "fx_stock_streaks(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                true,
            ),
            StockPlugin::LightRays => (
                ef::stock_light_rays().wgsl,
                "fx_stock_light_rays(uv, color, u.params[0], u.params[1], u.params[2], u.colors[0].rgb, RES)",
                true,
            ),
            StockPlugin::GodRays => (
                ef::stock_god_rays().wgsl,
                "fx_stock_god_rays(uv, color, u.params[0], u.params[1], u.params[2], u.colors[0].rgb, RES)",
                true,
            ),
            StockPlugin::LensFlare => (
                ef::stock_lens_flare().wgsl,
                "fx_stock_lens_flare(uv, color, u.params[0], u.params[1], u.params[2], u.colors[0].rgb, RES)",
                true,
            ),
            StockPlugin::Curves => (
                ef::stock_curves().wgsl,
                "fx_stock_curves(uv, color, u.params[0].xyzw, u.params[1].x)",
                false,
            ),
            StockPlugin::ColorBalance => (
                ef::stock_color_balance().wgsl,
                "fx_stock_color_balance(uv, color, vec3<f32>(u.params[0].x, u.params[0].y, u.params[0].z), vec3<f32>(u.params[0].w, u.params[1].x, u.params[1].y), vec3<f32>(u.params[1].z, u.params[1].w, u.params[2].x))",
                false,
            ),
            StockPlugin::ColorWheels => (
                ef::stock_color_wheels().wgsl,
                "fx_stock_color_wheels(uv, color, u.params[0].xyz)",
                false,
            ),
            StockPlugin::LightLeak => (
                ef::stock_light_leak().wgsl,
                "fx_stock_light_leak(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                false,
            ),
            StockPlugin::Solid => (
                ef::stock_solid().wgsl,
                "fx_stock_solid(uv, color, u.colors[0].rgb, u.params[0].x, u.params[0].y)",
                false,
            ),
            StockPlugin::GridGen => (
                ef::stock_grid().wgsl,
                "fx_stock_grid(uv, color, u.colors[0].rgb, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                false,
            ),
            StockPlugin::Shapes => (
                ef::stock_shapes().wgsl,
                "fx_stock_shapes(uv, color, u.colors[0].rgb, u.params[0].x, u.params[0].y, u.params[0].z, u.params[0].w, RES)",
                false,
            ),
            StockPlugin::Saber => (
                ef::stock_saber().wgsl,
                "fx_stock_saber(uv, color, u.params[0].x, u.params[0].y, u.params[1].x, u.params[1].y, u.params[1].z, u.params[1].w, u.params[2].x, u.params[2].y, u.params[2].z, u.params[2].w, u.colors[0].rgb, u.colors[1].rgb, u.colors[2].rgb, u.misc.x, RES)",
                false,
            ),
            StockPlugin::DirectionalBlur => (
                ef::stock_directional_blur_resample().wgsl,
                "fx_stock_directional_blur_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::MotionBlur => (
                ef::stock_motion_blur_resample().wgsl,
                "fx_stock_motion_blur_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            StockPlugin::RadialBlur => (
                ef::stock_radial_blur_resample().wgsl,
                "fx_stock_radial_blur_c(uv, color, u.params[0].x, u.params[0].y, 0.0, RES)",
                true,
            ),
            StockPlugin::ZoomBlur => (
                ef::stock_zoom_blur_resample().wgsl,
                "fx_stock_zoom_blur_c(uv, color, u.params[0].x, u.params[0].y, 1.0, RES)",
                true,
            ),
            StockPlugin::Bilateral => (
                ef::stock_bilateral_resample().wgsl,
                "fx_stock_bilateral_c(uv, color, u.params[0].x, u.params[0].y, RES)",
                true,
            ),
            _ => {
                return Err(format!("No native WGSL pass implemented for stock plugin {:?}", plugin));
            }
        };
        let label = format!("FxPass_{:?}", plugin);
        let call = call.replace("RES", res);
        Self::compile_resample(gpu, &label, target_format, body, &call, resample)
    }

    /// Compile a pass for a built-in (non-stock) evaluated effect by its
    /// stable id: point color ops plus the CPU-exact resampling twins
    /// (`displacement`, `warp`) from [`crate::effect_filters`].
    pub fn for_builtin(
        gpu: &GpuContext,
        id: &str,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        use crate::effect_filters as ef;
        // Fourth element: prepend the bilinear-exact resampling helper.
        let (body, call, resample): (&str, &str, bool) = match id {
            "brightness_contrast" => (
                ef::brightness_contrast().wgsl,
                "fx_brightness_contrast(uv, color, u.params[0].x, u.params[0].y)",
                false,
            ),
            "tint" => (
                ef::tint().wgsl,
                "fx_tint(uv, color, u.params[0].yzw, u.params[1].xyz, u.params[0].x)",
                false,
            ),
            "levels" => (
                ef::levels().wgsl,
                "fx_levels(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, u.params[0].w, u.params[1].x)",
                false,
            ),
            "hue_saturation" => (
                ef::hue_saturation().wgsl,
                "fx_hue_saturation(uv, color, u.params[0].x, u.params[0].y, u.params[0].z)",
                false,
            ),
            "invert" => (
                ef::invert().wgsl,
                "fx_invert(uv, color, u.params[0].x)",
                false,
            ),
            "exposure" => (
                ef::exposure().wgsl,
                "fx_exposure(uv, color, u.params[0].x)",
                false,
            ),
            "vibrance" => (
                ef::vibrance().wgsl,
                "fx_vibrance(uv, color, u.params[0].x)",
                false,
            ),
            "chroma_key" => (
                ef::chroma_key().wgsl,
                "fx_chroma_key(uv, color, u.params[0].xyz, u.params[0].w, u.params[1].x)",
                false,
            ),
            "luma_key" => (
                ef::luma_key().wgsl,
                "fx_luma_key(uv, color, u.params[0].x, u.params[0].y)",
                false,
            ),
            "swap_color" => (
                ef::swap_color().wgsl,
                "fx_swap_color(uv, color, u.params[0].xyz, u.params[1].xyz, u.params[2].x, u.params[2].y)",
                false,
            ),
            "vignette" => (
                ef::vignette().wgsl,
                "fx_vignette(uv, color, u.params[0].x, u.params[0].y)",
                false,
            ),
            "displacement" => (
                ef::displacement_resample().wgsl,
                "fx_displacement_c(uv, color, u.params[0].x, u.params[0].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "warp" => (
                ef::warp_wave().wgsl,
                "fx_warp_wave_c(uv, color, u.params[0].x, u.params[0].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "cel_shading" => (
                ef::cel_shade_resample().wgsl,
                "fx_cel_shade(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, u.params[0].w, u.colors[0].rgb, u.params[1].x, u.colors[1].rgb, u.params[1].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "oil_paint" => (
                ef::oil_paint_resample().wgsl,
                "fx_oil_paint(uv, color, u.params[0].x, u.params[0].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "match_color" => (
                ef::match_grade_resample().wgsl,
                "fx_match_grade(uv, color, u.params[0].x, u.colors[0].rgb, u.colors[1].rgb, u.colors[2].rgb, u.colors[3].rgb, vec3<f32>(u.params[0].y, u.params[0].z, u.params[0].w), vec3<f32>(u.params[1].x, u.params[1].y, u.params[1].z), 0.0)",
                false,
            ),
            "match_light" => (
                ef::match_grade_resample().wgsl,
                "fx_match_grade(uv, color, u.params[0].x, u.colors[0].rgb, u.colors[1].rgb, u.colors[2].rgb, u.colors[3].rgb, vec3<f32>(u.params[0].y, u.params[0].z, u.params[0].w), vec3<f32>(u.params[1].x, u.params[1].y, u.params[1].z), 1.0)",
                false,
            ),
            "inner_glow" => (
                ef::inner_glow_resample().wgsl,
                "fx_inner_glow(uv, color, u.params[0].x, u.params[0].y, u.colors[0].rgb, u.params[0].z, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "dither" => (
                ef::dither_filter().wgsl,
                "fx_dither(uv, color, u.params[0].x, u.params[0].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "rim_light" => (
                ef::rim_light_resample().wgsl,
                "fx_rim_light(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, u.colors[0].rgb, u.params[1].x, u.params[0].w, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            // NOTE: no "outer_glow" arm on purpose. Outer glow is a
            // blit-stage effect (blurred silhouette under content); a
            // chain twin would double-apply against the blit pass, and the
            // analytic twin cannot reproduce the blur anyway. It previews
            // on CPU (its silhouette blur already takes the GPU compute
            // path inside `blur_buffer`).
            "checkerboard" => (
                ef::checkerboard().wgsl,
                "fx_checkerboard(uv, color, u.params[0].xy, u.params[1].xyz, u.params[2].xyz, 1.0)",
                false,
            ),
            "gradient_ramp" => (
                ef::gradient_ramp().wgsl,
                "fx_gradient_ramp(uv, color, u.params[0].xyz, u.params[1].xyz, u.params[2].x, u.params[2].y)",
                false,
            ),
            _ => {
                return Err(format!("No native WGSL pass implemented for built-in effect {id}"));
            }
        };
        Self::compile_resample(gpu, &format!("FxPass_builtin_{id}"), target_format, body, call, resample)
    }

    /// Pointwise twin for one StockOp node in a Node Graph: (kernel body,
    /// call template, floats consumed). Only pure per-pixel kernels, and
    /// only twins that read no `u.colors` slots (StockOp nodes carry
    /// float params only). Mirrors the pointwise arms of `for_stock`.
    fn graph_stock_twin(plugin: StockPlugin) -> Option<(&'static str, &'static str, usize)> {
        use crate::effect_filters as ef;
        let (body, call, used): (&str, &str, usize) = match plugin {
            StockPlugin::Posterize => (
                ef::stock_posterize().wgsl,
                "fx_stock_posterize(uv, color, u.params[0].x)",
                1,
            ),
            StockPlugin::Threshold => (
                ef::stock_threshold().wgsl,
                "fx_stock_threshold(uv, color, u.params[0].x, u.params[0].y)",
                2,
            ),
            StockPlugin::FilmGrain => (
                ef::stock_film_grain().wgsl,
                "fx_stock_film_grain(uv, color, u.params[0].x, u.misc.x, u.params[0].y)",
                2,
            ),
            StockPlugin::Scanlines => (
                ef::stock_scanlines().wgsl,
                "fx_stock_scanlines(uv, color, u.params[0].x, u.params[0].y, u.misc.z)",
                2,
            ),
            StockPlugin::Crop => (
                ef::stock_crop().wgsl,
                "fx_stock_crop(uv, color, u.params[0], vec2<f32>(u.misc.y, u.misc.z))",
                4,
            ),
            StockPlugin::TemperatureTint => (
                ef::stock_temperature_tint().wgsl,
                "fx_stock_temperature_tint(uv, color, u.params[0].xy)",
                2,
            ),
            StockPlugin::SpillSuppress => (
                ef::stock_spill_suppress().wgsl,
                "fx_stock_spill_suppress(uv, color, u.params[0].x)",
                1,
            ),
            StockPlugin::DifferenceKey => (
                ef::stock_difference_key().wgsl,
                "fx_stock_difference_key(uv, color, u.params[0].x, u.params[0].y, u.params[0].z)",
                3,
            ),
            StockPlugin::Curves => (
                ef::stock_curves().wgsl,
                "fx_stock_curves(uv, color, u.params[0].xyzw, u.params[1].x)",
                5,
            ),
            StockPlugin::ColorBalance => (
                ef::stock_color_balance().wgsl,
                "fx_stock_color_balance(uv, color, vec3<f32>(u.params[0].x, u.params[0].y, u.params[0].z), vec3<f32>(u.params[0].w, u.params[1].x, u.params[1].y), vec3<f32>(u.params[1].z, u.params[1].w, u.params[2].x))",
                9,
            ),
            StockPlugin::ColorWheels => (
                ef::stock_color_wheels().wgsl,
                "fx_stock_color_wheels(uv, color, u.params[0].xyz)",
                3,
            ),
            StockPlugin::LightLeak => (
                ef::stock_light_leak().wgsl,
                "fx_stock_light_leak(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                3,
            ),
            StockPlugin::Halo => (
                ef::stock_halo().wgsl,
                "fx_stock_halo(uv, color, u.params[0].x, u.params[0].y, u.params[0].z, RES)",
                3,
            ),
            _ => return None,
        };
        Some((body, call, used))
    }

    /// Shift every `u.params[i]` index in a stock call template forward by
    /// `ofs` vec4 slots (the graph packs each StockOp's floats vec4-aligned).
    fn param_shift(call: &str, ofs: usize) -> String {
        let mut s = call.to_string();
        for i in (0..4).rev() {
            s = s.replace(&format!("u.params[{i}]"), &format!("u.params[{ofs}]"));
        }
        s
    }

    /// Compile a pass reproducing a color-clean Node Graph exactly: walk
    /// backwards from Output, emit one vec4 WGSL expression per node.
    /// Content (BaseValueIn) is the source pixel, Blend modes are inline
    /// elementwise formulas, StockOp reuses the pointwise kernel twins with
    /// vec4-aligned param slots. Anything else (Math/Lerp/Factor/Break,
    /// spatial kernels, >16 float params, cycles) declines with Err â€” the
    /// caller keeps that graph on the exact CPU path.
    pub fn for_graph(
        gpu: &GpuContext,
        graph: &project::ModifierGraph,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
                use project::{ConstantMode, NodeKind};
        let Some(out_id) = graph.nodes.iter().find(|n| matches!(n.kind, NodeKind::Output)).map(|n| n.id.clone()) else {
            return Err("graph has no Output node".into());
        };
        if !graph.is_color_clean() {
            return Err("graph is not color-clean".into());
        }


        struct Gs<'a> {
            graph: &'a project::ModifierGraph,
            bodies: Vec<&'static str>,
            /// User Wgsl node functions (one per unique code), emitted
            /// before `fx_graph_eval`.
            fns: Vec<String>,
            wgsl_memo: std::collections::HashMap<String, String>,
            wgsl_seq: usize,
            params: Vec<f32>,
            preamble: String,
            memo: std::collections::HashMap<String, String>,
            visiting: Vec<String>,
            stock_seq: usize,
        }

        impl<'a> Gs<'a> {
            /// Expression for one wire into `node_id`'s socket, if wired.
            fn wire(&mut self, node_id: &str, socket: &str) -> Result<Option<String>, String> {
                let hit = self
                    .graph
                    .connections
                    .iter()
                    .find(|c| c.to_node == node_id && c.to_socket == socket)
                    .map(|c| (c.from_node.clone(), c.from_socket.clone()));
                match hit {
                    Some((from, from_socket)) => Ok(Some(self.select(&from, &from_socket)?)),
                    None => Ok(None),
                }
            }

            /// Expression for one node output socket. Clean graphs only
            /// wire `value`/`result`/channel names: a vec4 bus or a `.c`
            /// component read (which broadcasts through WGSL scalar math).
            fn select(&mut self, node_id: &str, socket: &str) -> Result<String, String> {
                let bus = self.node(node_id)?;
                Ok(match socket {
                    "value" | "result" | "" => bus,
                    "r" | "x" => format!("({bus}).r"),
                    "g" | "y" => format!("({bus}).g"),
                    "b" | "z" => format!("({bus}).z"),
                    _ => format!("({bus}).a"),
                })
            }

            /// One vec4 WGSL expression per node, memoized.
            fn node(&mut self, node_id: &str) -> Result<String, String> {
                if let Some(e) = self.memo.get(node_id) {
                    return Ok(e.clone());
                }
                if self.visiting.iter().any(|v| v == node_id) {
                    return Err("cycle in color-clean graph".into());
                }
                self.visiting.push(node_id.to_string());
                let node = match self.graph.get_node(node_id) {
                    Some(n) => n.kind.clone(),
                    None => {
                        self.visiting.pop();
                        return Ok("color".to_string());
                    }
                };
                let res = self.node_kind(node_id, &node);
                self.visiting.pop();
                let expr = res?;
                self.memo.insert(node_id.to_string(), expr.clone());
                Ok(expr)
            }

            fn node_kind(&mut self, node_id: &str, kind: &project::NodeKind) -> Result<String, String> {
                use project::modifier::GraphBlendMode;
                Ok(match kind {
                    NodeKind::BaseValueIn => "color".to_string(),
                    NodeKind::Constant { mode, value, g, b, a } => {
                        // Mirror of the engine's constant_comp expansion.
                        let src = [*value, *g, *b, *a];
                        let v: [f32; 4] = match mode {
                            ConstantMode::Float | ConstantMode::Int => [src[0]; 4],
                            ConstantMode::Color => src,
                            ConstantMode::Vec2 => [src[0], src[1], 0.0, 1.0],
                            ConstantMode::Vec3 => [src[0], src[1], src[2], 1.0],
                        };
                        format!("vec4<f32>({:.9}, {:.9}, {:.9}, {:.9})", v[0], v[1], v[2], v[3])
                    }
                    NodeKind::Blend { mode, .. } => {
                        // a + (mode.mix(a,b) - a) * w — the CPU per-channel
                        // walk folds to elementwise vector math here.
                        let a = self.wire(node_id, "a")?.unwrap_or_else(|| "color".to_string());
                        let b = self.wire(node_id, "b")?.unwrap_or_else(|| "vec4<f32>(0.0)".to_string());
                        let w = self.wire(node_id, "weight")?.unwrap_or_else(|| "0.0".to_string());
                        let mix = match mode {
                            GraphBlendMode::Linear => b.clone(),
                            GraphBlendMode::Screen => {
                                format!("(1.0 - (1.0 - {a}) * (1.0 - {b}))")
                            }
                            GraphBlendMode::Multiply => format!("({a} * {b})"),
                            GraphBlendMode::Add => format!("({a} + {b})"),
                            GraphBlendMode::Subtract => format!("({a} - {b})"),
                            GraphBlendMode::Difference => format!("(abs({a} - {b}))"),
                            GraphBlendMode::Overlay => format!(
                                "(select(1.0 - 2.0 * (1.0 - {a}) * (1.0 - {b}), 2.0 * {a} * {b}, {a} < vec4<f32>(0.5)))"
                            ),
                        };
                        format!("(({a}) + ({mix} - ({a})) * ({w}))")
                    }
                    NodeKind::StockOp { plugin, params } => {
                        let Some((body, call, used)) = FxPass::graph_stock_twin(*plugin) else {
                            return Err(format!("no graph WGSL twin for {:?}", plugin));
                        };
                        if !self.bodies.contains(&body) {
                            self.bodies.push(body);
                        }
                        // Vec4-aligned param slots, resolved against the
                        // descriptor defaults exactly like the CPU.
                        let resolved = project::stock::stock_params_resolved(*plugin, params);
                        let slot = self.params.len().div_ceil(4);
                        self.params.extend_from_slice(&resolved[..used]);
                        while !self.params.len().is_multiple_of(4) {
                            self.params.push(0.0);
                        }
                        let var = format!("s{}", self.stock_seq);
                        self.stock_seq += 1;
                        let input = self
                            .wire(node_id, "in")?
                            .unwrap_or_else(|| "color".to_string());
                        self.preamble.push_str(&format!("    var {var} = {input};\n"));
                        let templ = FxPass::param_shift(call, slot)
                            .replace("(uv, color,", &format!("(uv, {var},"))
                            .replace("RES", "vec2<f32>(u.misc.y, u.misc.z)");
                        templ
                    }
                    NodeKind::Output => self.wire(node_id, "result")?.unwrap_or_else(|| "color".to_string()),
                    NodeKind::Wgsl { code } => {
                        if code.trim().is_empty() {
                            return Err("Wgsl node has no code".into());
                        }
                        // One user fn per unique code; their bodies must
                        // `return vec4<f32>(...)` (WGSL).
                        let name = match self.wgsl_memo.get(code) {
                            Some(n) => n.clone(),
                            None => {
                                let n = format!("wgsl_user_fn_{}", self.wgsl_seq);
                                self.wgsl_seq += 1;
                                self.wgsl_memo.insert(code.clone(), n.clone());
                                self.fns.push(format!(
                                    "fn {n}(color: vec4<f32>, uv: vec2<f32>, p: vec4<f32>) -> vec4<f32> {{\n{}\n}}\n",
                                    code
                                ));
                                n
                            }
                        };
                        let p0 = self.wire(node_id, "p0")?.unwrap_or_else(|| "0.0".to_string());
                        let p1 = self.wire(node_id, "p1")?.unwrap_or_else(|| "0.0".to_string());
                        let p2 = self.wire(node_id, "p2")?.unwrap_or_else(|| "0.0".to_string());
                        let p3 = self.wire(node_id, "p3")?.unwrap_or_else(|| "0.0".to_string());
                        format!("{}(color, uv, vec4<f32>({}, {}, {}, {}))", name, p0, p1, p2, p3)
                    }
                    NodeKind::Transform { x, y, scale, rotation } => {
                        let sc = if *scale == 0.0 { 1.0 } else { *scale };
                        format!(
                            "textureSample(src_tex, src_sampler, (mat2x2<f32>(cos({:.9}), sin({:.9}), -sin({:.9}), cos({:.9})) * ((uv - vec2<f32>(0.5)) / vec2<f32>({sc:.9}))) + vec2<f32>(0.5) - vec2<f32>({:.9}, {:.9}))",
                            rotation, rotation, rotation, rotation, x, y
                        )
                    }
                    NodeKind::Tile { repeat_x, repeat_y } => {
                        format!(
                            "textureSample(src_tex, src_sampler, fract(uv * vec2<f32>({:.9}, {:.9})))",
                            repeat_x, repeat_y
                        )
                    }
                    NodeKind::Glow { radius, intensity } => {
                        if !self.bodies.contains(&FX_RING_WGSL) {
                            self.bodies.push(FX_RING_WGSL);
                        }
                        let c = self
                            .wire(node_id, "color")?
                            .unwrap_or_else(|| "color".to_string());
                        format!("(({c}) + fx_ring(uv, {:.9}) * {:.9})", radius, intensity)
                    }
                    NodeKind::Bloom { threshold, radius, intensity } => {
                        if !self.bodies.contains(&FX_BRIGHT_RING_WGSL) {
                            self.bodies.push(FX_BRIGHT_RING_WGSL);
                        }
                        let c = self
                            .wire(node_id, "color")?
                            .unwrap_or_else(|| "color".to_string());
                        format!(
                            "(({c}) + fx_bright_ring(uv, {:.9}, {:.9}) * {:.9})",
                            radius, threshold, intensity
                        )
                    }
                    _ => return Err(format!("graph node not twinable: {:?}", kind)),
                })
            }
        }

        let mut gs = Gs {
            graph,
            bodies: Vec::new(),
            fns: Vec::new(),
            wgsl_memo: std::collections::HashMap::new(),
            wgsl_seq: 0,
            params: Vec::new(),
            preamble: String::new(),
            memo: std::collections::HashMap::new(),
            visiting: Vec::new(),
            stock_seq: 0,
        };
        let expr = gs.select(&out_id, "result")?;
        // FxUniforms packs 16 float params (4x vec4); anything wider
        // declines to the CPU path.
        if gs.params.len() > 16 {
            return Err(format!("graph params need {} floats > 16 budget", gs.params.len()));
        }
        let mut body = String::new();
        for b in &gs.bodies {
            body.push_str(b);
            body.push('\n');
        }
        for f in &gs.fns {
            body.push_str(f);
        }
        body.push_str(&format!(
            "fn fx_graph_eval(color: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {{\n{}    return {};\n}}\n",
            gs.preamble, expr
        ));
        let mut pass = Self::compile(
            gpu,
            "FxPass_graph",
            target_format,
            &body,
            "fx_graph_eval(color, uv)",
            &[],
        )?;
        pass.graph_params = gs.params;
        Ok(pass)
    }
    fn compile(
        gpu: &GpuContext,
        label: &str,
        target_format: wgpu::TextureFormat,
        body: &str,
        call: &str,
        uv_calls: &[&str],
    ) -> Result<Self, String> {
        let fragment_src = fx_fragment_src(body, uv_calls, call);
        let full_src = format!("{FX_VERT}\n{fragment_src}");
        Self::new(gpu, target_format, label, &full_src)
    }

    /// Compile with the bilinear-exact resampling helper prepended when
    /// `resample` is set (sampling twins need it; point twins omit it).
    fn compile_resample(
        gpu: &GpuContext,
        label: &str,
        target_format: wgpu::TextureFormat,
        body: &str,
        call: &str,
        resample: bool,
    ) -> Result<Self, String> {
        if !resample {
            return Self::compile(gpu, label, target_format, body, call, &[]);
        }
        let owned = format!("{}\n{body}", crate::effect_filters::FX_RESAMPLE_WGSL);
        Self::compile(gpu, label, target_format, &owned, call, &[])
    }

    /// Record the pass into `encoder`: `source_view` -> `target` with packed uniforms.
    pub fn record_into(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        source_view: &wgpu::TextureView,
        target: &RenderTarget,
        uniforms: FxUniforms,
    ) {
        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Fx Uniform Buffer"),
            size: std::mem::size_of::<FxUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fx Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Fx Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Run the pass: `source_view` -> `target` with packed uniforms.
    pub fn render(
        &self,
        gpu: &GpuContext,
        source_view: &wgpu::TextureView,
        target: &RenderTarget,
        uniforms: FxUniforms,
    ) {
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Fx Command Encoder"),
        });
        self.record_into(gpu, &mut encoder, source_view, target, uniforms);
        gpu.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Identity blit: copies one target into another inside a multi-stage
    /// chain (landing a merge result back in the chain cursor).
    pub fn blit(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        const BODY: &str = r#"fn fx_blit(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    return color;
}"#;
        Self::compile(
            gpu,
            "FxPass_blit",
            target_format,
            BODY,
            "fx_blit(uv, color)",
            &[],
        )
    }

    /// Bright-pass extract shared by the spatial bloom stage: pixels whose
    /// premultiplied luminance clears 0.45 pass through, everything else
    /// goes transparent (mirrors `apply_bloom`).
    pub fn bright_extract(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        const BODY: &str = r#"fn fx_bright_extract(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let lum = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114)) * color.a;
    if (lum > 0.45) {
        return color;
    }
    return vec4<f32>(0.0);
}"#;
        Self::compile(
            gpu,
            "FxPass_bright_extract",
            target_format,
            BODY,
            "fx_bright_extract(uv, color)",
            &[],
        )
    }

    /// Long-shadow march stage for the multi-stage stock Long Shadow path:
    /// extrusion mask with fade + tint (see `effect_filters::
    /// stock_long_shadow_march`). Params/colors travel in the shared
    /// uniforms; needs the resampling helper for the mask taps.
    pub fn shadow_march(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        Self::compile_resample(
            gpu,
            "FxPass_shadow_march",
            target_format,
            crate::effect_filters::stock_long_shadow_march().wgsl,
            "fx_stock_long_shadow_march(uv, color, vec2<f32>(u.misc.y, u.misc.z))",
            true,
        )
    }

    /// Outer-glow aura fill for the multi-stage built-in aura path:
    /// spread-boosted PURE-ALPHA silhouette (black rgb like the CPU
    /// `sil` buffer), feeding the shared compute blur; the under-merge
    /// applies the authored tint at composite time (exactly the CPU
    /// blur-then-colorize order â€” colorizing after the sigma blur).
    pub fn glow_fill(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        const BODY: &str = r#"fn fx_outer_glow_fill(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let boost = select(1.0, 1.0 + u.params[0].x / 20.0, u.params[0].x > 5.0);
    let a = min(color.a * boost, 1.0) * clamp(u.params[0].y, 0.0, 1.0);
    if (a <= 0.002) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(0.0, 0.0, 0.0, a);
}"#;
        Self::compile(
            gpu,
            "FxPass_glow_fill",
            target_format,
            BODY,
            "fx_outer_glow_fill(uv, color)",
            &[],
        )
    }

    /// Drop-shadow aura fill: pure-alpha silhouette sampled at the shadow
    /// offset via the resample helper (offset in px â€” blur commutes with
    /// the constant offset, so sampling before blurring matches the CPU
    /// order). The under-merge applies the tint.
    pub fn shadow_fill(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        const BODY: &str = r#"fn fx_drop_shadow_fill(uv: vec2<f32>, color: vec4<f32>, res: vec2<f32>) -> vec4<f32> {
    let px = uv * res - vec2<f32>(0.5);
    let s = fx_sample_cpu(px - vec2<f32>(u.params[0].x, u.params[0].y), res);
    let a = s.a * clamp(u.params[0].z, 0.0, 1.0);
    if (a <= 0.002) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(0.0, 0.0, 0.0, a);
}"#;
        Self::compile_resample(
            gpu,
            "FxPass_shadow_fill",
            target_format,
            BODY,
            "fx_drop_shadow_fill(uv, color, vec2<f32>(u.misc.y, u.misc.z))",
            true,
        )
    }
}

/// Two-texture merge pass: binary fullscreen ops (unsharp mix, screen)
/// that need the original AND a derived buffer side by side. Same
/// fullscreen triangle + `FxUniforms` block as [`FxPass`]; bindings are
/// `tex_a(0)`, `tex_b(1)`, sampler(2), uniforms(3).
pub struct FxMergePass {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl FxMergePass {
    /// Build from a transparency gate plus the merge expression assigning
    /// the output color (`o` is the primary sample, `b` the secondary).
    fn from_calls(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
        label: &str,
        gate: &str,
        merge_call: &str,
    ) -> Result<Self, String> {
        const BINDINGS: &str = r#"
struct FxUniforms {
    params: array<vec4<f32>, 4>,
    colors: array<vec4<f32>, 4>,
    misc: vec4<f32>,
};

@group(0) @binding(0) var tex_a: texture_2d<f32>;
@group(0) @binding(1) var tex_b: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> u: FxUniforms;
"#;
        let src = format!(
            "{FX_VERT}\n{BINDINGS}\n@fragment\nfn fs_main(in: VsOut) -> @location(0) vec4<f32> {{\n    let o = textureSample(tex_a, samp, in.uv);\n    let b = textureSample(tex_b, samp, in.uv);\n    if ({gate}) {{\n        return o;\n    }}\n    return {merge_call};\n}}\n"
        );
        naga::front::wgsl::parse_str(&src)
            .map_err(|e| format!("fx merge pass {label} WGSL invalid: {e:?}"))?;
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(src.into()),
        });
        let bind_group_layout =
            gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout =
            gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Ok(Self { pipeline, bind_group_layout, sampler })
    }

    /// Unsharp-mask merge: `orig + (orig - blurred) * k`, alpha preserved
    /// (mirrors `apply_sharpen`).
    pub fn unsharp(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        Self::from_calls(
            gpu,
            target_format,
            "FxMerge_unsharp",
            "o.a <= 0.0",
            "vec4<f32>(clamp(o.rgb + (o.rgb - b.rgb) * u.params[0].x, vec3<f32>(0.0), vec3<f32>(1.0)), o.a)",
        )
    }

    /// Screen merge for the bloom stage: `1 - (1-o)(1-g*k)` with the glow
    /// alpha gate (mirrors `apply_bloom`).
    pub fn screen(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        Self::from_calls(
            gpu,
            target_format,
            "FxMerge_screen",
            "o.a <= 0.0 || b.a <= 0.0",
            "vec4<f32>(clamp(vec3<f32>(1.0) - (vec3<f32>(1.0) - o.rgb) * (vec3<f32>(1.0) - b.rgb * u.params[0].x), vec3<f32>(0.0), vec3<f32>(1.0)), o.a)",
        )
    }

    /// Long-shadow under-merge: source over the blurred shadow layer
    /// (`u.params[0].x` = mode 0 behind / 1 cutout / 2 only). The shadow
    /// layer holds straight bytes (tint, alpha â€” exactly the CPU
    /// `to_rgba8` feed), so the composite re-premultiplies explicitly â€”
    /// mirroring `k_long_shadow`'s odd-but-exact `d + s * (1 - s.a)` form
    /// rather than "fixing" it.
    pub fn shadow_under(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        // Gate skips only for behind/cutout (shadow-only must emit the
        // shadow layer untouched, including its transparent pixels).
        Self::from_calls(
            gpu,
            target_format,
            "FxMerge_shadow_under",
            "b.a <= 0.0 && u.params[0].x < 1.5",
            "select(select(vec4<f32>((o.rgb * o.a + b.rgb * b.a * (1.0 - b.a)) / max(o.a + b.a * (1.0 - o.a), 0.000001), o.a + b.a * (1.0 - o.a)), b, u.params[0].x >= 1.5), o, u.params[0].x >= 0.5 && u.params[0].x < 1.5 && o.a > 0.01)",
        )
    }

    /// Aura under-merge: content over the blurred aura silhouette. The
    /// aura holds pure-alpha bytes (black rgb â€” the CPU `sil` blur
    /// feed), so this colorizes with `u.colors[0]` AT composite time,
    /// mirroring the CPU colorize-after-blur order; bytes land normalized
    /// back to straight alpha for the chain cursor.
    pub fn under_prem(
        gpu: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        // Gates: no aura (pure content passthrough) and full content
        // coverage (aura fully occluded â€” mirrors src-over's 1 - o.a â‰ˆ 0).
        Self::from_calls(
            gpu,
            target_format,
            "FxMerge_under_prem",
            "b.a <= 0.0 || o.a >= 0.999",
            "vec4<f32>((o.rgb * o.a + u.colors[0].rgb * b.a * (1.0 - o.a)) / max(o.a + b.a * (1.0 - o.a), 0.000001), o.a + b.a * (1.0 - o.a))",
        )
    }

    /// Record the merge into `encoder`: `(view_a, view_b)` -> `target`.
    pub fn record_into(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        view_a: &wgpu::TextureView,
        view_b: &wgpu::TextureView,
        target: &RenderTarget,
        uniforms: FxUniforms,
    ) {
        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Fx Merge Uniform Buffer"),
            size: std::mem::size_of::<FxUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fx Merge Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view_a),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view_b),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Fx Merge Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Stock plug-ins whose native pass reproduces the CPU kernel within u8
/// rounding (gentle point/rect math, no time seeds, plus the bilinear-
/// exact resampling twins): the set the live GPU chain may take without
/// visual change. Excluded despite compiling: FilmGrain (frame-seeded CPU
/// vs time-seeded twin), Threshold (steep smoothstep transfer curves
/// amplify u8 rounding through cascades), and Halftone (luminance-
/// thresholded binary dots flip on u8-quantized smooth gradients â€” the
/// twin is exact, but the chain cannot bound it); those stay on the exact
/// CPU path, as does export. Saber is excluded too: its twin is a fixed
/// beam generator while the CPU kernel is mask-driven with a blur-based
/// glow, so the two cannot agree â€” it needs a multi-stage (blur + merge)
/// pass before it can rejoin this set. Stock Glow is excluded as well:
/// threshold presence flips across the u8 boundary while the white
/// screen amplifies any nonzero mask to full strength â€” unboundable in
/// a u8 chain for the same reason as Threshold.
///
/// Blur family: Directional/Motion/Radial/Zoom/Bilateral have exact
/// fragment twins (premultiplied gather, unpremultiplied output;
/// bilateral fuses both range-weighted passes in one evaluation so no
/// u8 intermediate ever carries the alpha-normalized luminance the
/// weights read). Box and Defocus run the separable box compute path
/// (`ComputeBoxPipeline`) â€” no fragment twins, so they are members here
/// but stay out of [`stock_wgsl_plugins`]. Bokeh stays on the exact CPU
/// path: its blurred-highlight alpha tail below 1/255 carries
/// full-strength screen presence on the CPU but quantizes to zero in any
/// u8 chain (same family as the Glow decline).
pub const CHAIN_SAFE_STOCK: [StockPlugin; 46] = [
    StockPlugin::Posterize,
    StockPlugin::TemperatureTint,
    StockPlugin::SpillSuppress,
    StockPlugin::Crop,
    StockPlugin::Scanlines,
    StockPlugin::DifferenceKey,
    StockPlugin::Wave,
    StockPlugin::Ripple,
    StockPlugin::Twirl,
    StockPlugin::Bulge,
    StockPlugin::Spherize,
    StockPlugin::LensDistortion,
    StockPlugin::Mirror,
    StockPlugin::Repeat,
    StockPlugin::Offset,
    StockPlugin::Pixelate,
    StockPlugin::Mosaic,
    StockPlugin::ChromaticAberration,
    StockPlugin::RgbSplit,
    StockPlugin::Emboss,
    StockPlugin::EdgeDetect,
    StockPlugin::Cartoon,
    StockPlugin::Card3d,
    StockPlugin::Curves,
    StockPlugin::ColorBalance,
    StockPlugin::ColorWheels,
    StockPlugin::LightLeak,
    StockPlugin::Solid,
    StockPlugin::GridGen,
    StockPlugin::Shapes,
    StockPlugin::CornerPin,
    StockPlugin::Reframe,
    StockPlugin::Liquify,
    StockPlugin::LongShadow,
    StockPlugin::Halo,
    StockPlugin::Streaks,
    StockPlugin::LightRays,
    StockPlugin::GodRays,
    StockPlugin::LensFlare,
    StockPlugin::DirectionalBlur,
    StockPlugin::MotionBlur,
    StockPlugin::RadialBlur,
    StockPlugin::ZoomBlur,
    StockPlugin::BoxBlur,
    StockPlugin::Defocus,
    StockPlugin::Bilateral,
];

/// Built-in (non-stock) evaluated effects whose WGSL twin reproduces the
/// CPU kernel within u8 rounding (gentle point math, no time seeds, no
/// spatial resampling): the set the live GPU chain may take without
/// visual change. Same audit bar as [`CHAIN_SAFE_STOCK`].
pub const CHAIN_SAFE_BUILTIN: [&str; 20] = [
    "brightness_contrast",
    "tint",
    "levels",
    "hue_saturation",
    "invert",
    "exposure",
    "vibrance",
    "chroma_key",
    "luma_key",
    "swap_color",
    "vignette",
    "displacement",
    "warp",
    "oil_paint",
    "checkerboard",
    "gradient_ramp",
    "match_color",
    "match_light",
    "inner_glow",
    "rim_light",
];

/// Stable built-in id for a GPU-ported evaluated effect, if any.
pub fn builtin_gpu_id(effect: &compositor::EvaluatedEffectType) -> Option<&'static str> {
    use compositor::EvaluatedEffectType as E;
    match effect {
        E::BrightnessContrast { .. } => Some("brightness_contrast"),
        E::Tint { .. } => Some("tint"),
        E::Levels { .. } => Some("levels"),
        E::HueSaturation { .. } => Some("hue_saturation"),
        E::Invert { .. } => Some("invert"),
        E::Exposure { .. } => Some("exposure"),
        E::Vibrance { .. } => Some("vibrance"),
        E::ChromaKey { .. } => Some("chroma_key"),
        E::LumaKey { .. } => Some("luma_key"),
        E::SwapColor { .. } => Some("swap_color"),
        E::Vignette { .. } => Some("vignette"),
        E::DisplacementMap { .. } => Some("displacement"),
        E::Warp { .. } => Some("warp"),
        E::OilPaint { .. } => Some("oil_paint"),
        E::MatchColor { .. } => Some("match_color"),
        E::MatchLight { .. } => Some("match_light"),
        E::InnerGlow { .. } => Some("inner_glow"),
        E::RimLight { .. } => Some("rim_light"),
        // NOTE: no "dither" arm on purpose. Ordered thresholds flip on
        // u8-quantized gradients (1/(levels-1) per flipped bit), the same
        // reason halftone/cel stay CPU. Its twin still compiles (honest
        // badge: no GPU claim).
        E::Checkerboard { .. } => Some("checkerboard"),
        E::GradientRamp { .. } => Some("gradient_ramp"),
        _ => None,
    }
}

/// Which stock plug-ins have native WGSL twins (the rest are multi-tap
/// spatial passes that run on the CPU convolution path, like blur).
pub fn stock_wgsl_plugins() -> Vec<StockPlugin> {
    use StockPlugin as S;
    vec![
        S::Curves,
        S::ColorBalance,
        S::ColorWheels,
        S::TemperatureTint,
        S::Posterize,
        S::Threshold,
        S::DifferenceKey,
        S::SpillSuppress,
        S::LightLeak,
        S::Scanlines,
        S::FilmGrain,
        S::FractalNoise,
        S::Dust,
        S::Scratches,
        S::Flicker,
        S::Halftone,
        S::Solid,
        S::FractalGen,
        S::GridGen,
        S::Shapes,
        S::PlasmaGen,
        S::Particles,
        S::Crop,
        S::Mosaic,
        S::Mirror,
        S::Repeat,
        S::Offset,
        S::Wave,
        S::Ripple,
        S::Twirl,
        S::Bulge,
        S::Spherize,
        S::LensDistortion,
        S::Pixelate,
        S::CornerPin,
        S::MeshWarp,
        S::Reframe,
        S::Liquify,
        S::Card3d,
        S::Saber,
        S::DirectionalBlur,
        S::MotionBlur,
        S::RadialBlur,
        S::ZoomBlur,
        S::Bilateral,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect_filters as ef;

    #[test]
    fn uniforms_pack_roundtrip() {
        let u = FxUniforms::pack(&[1.0, 2.0, 3.0], &[[1.0, 0.0, 0.0, 1.0]], 0.5, 1920.0, 1080.0, 7.0);
        assert_eq!(u.params[0], [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(u.colors[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(u.colors[3], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(u.misc, [0.5, 1920.0, 1080.0, 7.0]);
        assert_eq!(std::mem::size_of::<FxUniforms>(), 144);
    }

    #[test]
    fn every_wgsl_plugin_has_a_filter() {
        let color_ids: Vec<&str> = ef::all_color_filters().iter().map(|f| f.id).collect();
        let uv_ids: Vec<&str> = ef::all_uv_filters().iter().map(|f| f.id).collect();
        let resample_ids: Vec<&str> = ef::resample_filters().iter().map(|f| f.id).collect();
        // Slug of the plug-in id must appear in one of the registries
        // (resampling twins carry a `_resample` suffix).
        for plugin in stock_wgsl_plugins() {
            let slug = plugin.plugin_id().rsplit('.').next().unwrap();
            let want = format!("stock_{slug}");
            let want_resample = format!("stock_{slug}_resample");
            assert!(
                color_ids.contains(&want.as_str())
                    || uv_ids.contains(&want.as_str())
                    || resample_ids.contains(&want_resample.as_str()),
                "no WGSL twin for {}",
                plugin.plugin_id()
            );
        }
    }

    #[test]
    fn fragment_composer_parses() {
        let src = fx_fragment_src(
            ef::stock_posterize().wgsl,
            &[],
            "fx_stock_posterize(uv, color, u.params[0].x)",
        );
        let full = format!("{FX_VERT}\n{src}");
        naga::front::wgsl::parse_str(&full).expect("composed fx pass parses");
    }

    /// The four spatial node kinds must emit valid WGSL (ring helpers,
    /// fract wrap, rotation matrix). Skips where no wgpu device exists.
    #[test]
    fn spatial_graph_twins_compile() {
        use project::modifier::{ModifierGraph, ModifierNode, NodeConnection, NodeKind};
        let Ok(gpu) = GpuContext::new_headless() else { return };
        let cases: Vec<ModifierGraph> = vec![
            // Transform/Tile: source-only graphs (no color input).
            ModifierGraph {
                nodes: vec![
                    ModifierNode::new("fx", 0.0, 0.0, NodeKind::Transform { x: 0.1, y: -0.2, scale: 1.5, rotation: 30.0 }),
                    ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
                ],
                connections: vec![NodeConnection::new("fx", "value", "out", "result")],
            },
            ModifierGraph {
                nodes: vec![
                    ModifierNode::new("fx", 0.0, 0.0, NodeKind::Tile { repeat_x: 3.0, repeat_y: 2.0 }),
                    ModifierNode::new("out", 200.0, 0.0, NodeKind::Output),
                ],
                connections: vec![NodeConnection::new("fx", "value", "out", "result")],
            },
            // Glow/Bloom: color-wired graphs (ring helpers must parse).
            ModifierGraph {
                nodes: vec![
                    ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                    ModifierNode::new("fx", 120.0, 0.0, NodeKind::Glow { radius: 0.05, intensity: 1.5 }),
                    ModifierNode::new("out", 300.0, 0.0, NodeKind::Output),
                ],
                connections: vec![
                    NodeConnection::new("in", "value", "fx", "color"),
                    NodeConnection::new("fx", "value", "out", "result"),
                ],
            },
            ModifierGraph {
                nodes: vec![
                    ModifierNode::new("in", 0.0, 0.0, NodeKind::BaseValueIn),
                    ModifierNode::new("fx", 120.0, 0.0, NodeKind::Bloom { threshold: 0.6, radius: 0.08, intensity: 2.0 }),
                    ModifierNode::new("out", 300.0, 0.0, NodeKind::Output),
                ],
                connections: vec![
                    NodeConnection::new("in", "value", "fx", "color"),
                    NodeConnection::new("fx", "value", "out", "result"),
                ],
            },
        ];
        for graph in cases {
            let kind = graph.nodes.iter().find(|n| n.id == "fx").unwrap().kind.title();
            FxPass::for_graph(&gpu, &graph, wgpu::TextureFormat::Rgba8Unorm)
                .unwrap_or_else(|e| panic!("{kind} graph must compile: {e}"));
        }
    }

    /// Every `for_stock` arm must compile to a real pipeline (arity slips
    /// used to fail here and were swallowed by `let _` at the call site).
    /// Skips gracefully where no wgpu device exists at all.
    #[test]
    fn every_for_stock_arm_compiles() {
        use StockPlugin as S;
        let Ok(gpu) = GpuContext::new_headless() else { return };
        for plugin in [
            S::Posterize,
            S::Threshold,
            S::FilmGrain,
            S::Scanlines,
            S::Crop,
            S::TemperatureTint,
            S::SpillSuppress,
            S::DifferenceKey,
            S::Wave,
            S::Ripple,
            S::Twirl,
            S::Bulge,
            S::Spherize,
            S::LensDistortion,
            S::Mirror,
            S::Repeat,
            S::Offset,
            S::Pixelate,
            S::Mosaic,
            S::ChromaticAberration,
            S::RgbSplit,
            S::Emboss,
            S::EdgeDetect,
            S::Cartoon,
            S::Halftone,
            S::Card3d,
            S::CornerPin,
            S::Reframe,
            S::Liquify,
            S::Curves,
            S::ColorBalance,
            S::ColorWheels,
            S::LightLeak,
            S::Solid,
            S::GridGen,
            S::Shapes,
            S::Saber,
            S::Halo,
            S::Streaks,
            S::LightRays,
            S::GodRays,
            S::LensFlare,
        ] {
            FxPass::for_stock(&gpu, plugin, wgpu::TextureFormat::Rgba8Unorm)
                .unwrap_or_else(|e| panic!("for_stock {plugin:?} failed: {e}"));
        }
    }

    /// Every `for_builtin` arm must compile to a real pipeline, same
    /// bar as the stock arms above. Skips where no wgpu device exists.
    #[test]
    fn every_for_builtin_arm_compiles() {
        let Ok(gpu) = GpuContext::new_headless() else { return };
        for id in CHAIN_SAFE_BUILTIN.into_iter().chain(["cel_shading"]) {
            FxPass::for_builtin(&gpu, id, wgpu::TextureFormat::Rgba8Unorm)
                .unwrap_or_else(|e| panic!("for_builtin {id} failed: {e}"));
        }
        // Merge machinery variants compile (screen doubles for bloom-style
        // stages; shadow_under serves the stock Long Shadow path).
        FxMergePass::unsharp(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("unsharp compiles");
        FxMergePass::screen(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("screen compiles");
        FxMergePass::shadow_under(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("shadow_under compiles");
        FxPass::bright_extract(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("bright compiles");
        FxPass::shadow_march(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("march compiles");
        FxPass::blit(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("blit compiles");
    }

    /// Identity blit round-trip over a gradient: upload + linear sample +
    /// store must preserve bytes (isolates transport from twin math).
    #[test]
    fn blit_round_trips_gradient_bytes() {
        use crate::device::RenderTarget;
        let Ok(gpu) = GpuContext::new_headless() else { return };
        let fmt = wgpu::TextureFormat::Rgba8Unorm;
        // Identity blit round-trip over a gradient: upload + linear
        // sample + store must preserve bytes (isolates transport from
        // twin math).
        let big = RenderTarget::with_format(&gpu, 64, 64, fmt).expect("big");
        let bigout = RenderTarget::with_format(&gpu, 64, 64, fmt).expect("bigout");
        let mut grad = vec![0u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let i = (y * 64 + x) * 4;
                grad[i] = (x as f32 / 63.0 * 255.0).round() as u8;
                grad[i + 1] = (y as f32 / 63.0 * 255.0).round() as u8;
                grad[i + 2] = 102u8;
                grad[i + 3] = 255u8;
            }
        }
        big.write_texture_rgba(&gpu, &grad);
        let blitpass = FxPass::blit(&gpu, fmt).expect("blit compiles");
        let bu = FxUniforms::pack(&[], &[], 0.0, 64.0, 64.0, 0.0);
        blitpass.render(&gpu, big.view(), &bigout, bu);
        let pxc = bigout.read_texture_to_cpu(&gpu).expect("readback");
        let j = (11 * 64 + 32) * 4;
        assert_eq!(&pxc[j..j + 4], &[130u8, 45, 102, 255], "blit must round-trip, got {:?}", &pxc[j..j + 4]);
    }

    /// Fragment passes must agree with the CPU row convention (row 0 =
    /// top): cropping the top half clears the first rows, not the last.
    #[test]
    fn fx_pass_preserves_orientation() {
        use crate::device::RenderTarget;
        use StockPlugin as S;
        let Ok(gpu) = GpuContext::new_headless() else { return };
        let pass = FxPass::for_stock(&gpu, S::Crop, wgpu::TextureFormat::Rgba8Unorm)
            .expect("crop pass compiles");
        let src = RenderTarget::with_format(&gpu, 4, 4, wgpu::TextureFormat::Rgba8Unorm)
            .expect("src target");
        let dst = RenderTarget::with_format(&gpu, 4, 4, wgpu::TextureFormat::Rgba8Unorm)
            .expect("dst target");
        src.write_texture_rgba(&gpu, &[255u8; 4 * 4 * 4]);
        let uniforms = FxUniforms::pack(&[0.0, 50.0, 0.0, 0.0], &[], 0.0, 4.0, 4.0, 0.0);
        pass.render(&gpu, src.view(), &dst, uniforms);
        let px = dst.read_texture_to_cpu(&gpu).expect("readback");
        assert_eq!(px.len(), 4 * 4 * 4);
        // Top rows cleared, bottom rows intact.
        assert_eq!(&px[0..8], &[0u8; 8], "row 0 must clear, got {:?}", &px[0..16]);
        assert_eq!(&px[48..64], &[255u8; 16], "row 3 must survive, got {:?}", &px[48..64]);
    }
}
