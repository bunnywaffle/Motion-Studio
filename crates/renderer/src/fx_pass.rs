//! Native wgpu effect passes: run any WGSL filter on the GPU.
//!
//! [`FxPass`] is a generic fullscreen-triangle pass: source texture +
//! sampler + one uniform block. Each stock effect maps to a WGSL body plus
//! packed uniforms via [`fx_source_for`] / [`pack_stock_uniforms`], so the
//! whole catalogue runs natively on wgpu (export path) with the same math
//! the CPU preview uses. Multi-tap spatial effects (blurs, morphology,
//! unsharp) run as multi-pass or CPU convolution — see each kernel's docs.

use crate::device::{GpuContext, RenderTarget};
use bytemuck::{Pod, Zeroable};
use project::StockPlugin;

/// Uniform block shared by every stock WGSL twin: 16 param floats, then
/// `(time_s, width_px, height_px, seed)`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct FxUniforms {
    pub params: [[f32; 4]; 4],
    pub misc: [f32; 4],
}

impl FxUniforms {
    /// Pack evaluated scalar params + frame context into the block.
    pub fn pack(params: &[f32], time_s: f32, w: f32, h: f32, seed: f32) -> Self {
        let mut p = [[0.0f32; 4]; 4];
        for (i, v) in params.iter().take(16).enumerate() {
            p[i / 4][i % 4] = *v;
        }
        Self { params: p, misc: [time_s, w, h, seed] }
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
    misc: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: FxUniforms;
"#;

/// Build a complete fragment-stage source for one effect body: samples the
/// source texture, runs `main_call` (which must assign `color`), and
/// returns it. `uv_calls` run first for pre-sample remaps.
pub fn fx_fragment_src(effect_body: &str, uv_calls: &[&str], main_call: &str) -> String {
    let mut src = String::from(FX_BINDINGS_WGSL);
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
        Ok(Self { pipeline, bind_group_layout, sampler })
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
                "fx_cel_shade(uv, color, u.params[0].x, u.params[0].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            "oil_paint" => (
                ef::oil_paint_resample().wgsl,
                "fx_oil_paint(uv, color, u.params[0].x, u.params[0].y, vec2<f32>(u.misc.y, u.misc.z))",
                true,
            ),
            _ => {
                return Err(format!("No native WGSL pass implemented for built-in effect {id}"));
            }
        };
        Self::compile_resample(gpu, &format!("FxPass_builtin_{id}"), target_format, body, call, resample)
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
/// thresholded binary dots flip on u8-quantized smooth gradients — the
/// twin is exact, but the chain cannot bound it); those stay on the exact
/// CPU path, as does export.
pub const CHAIN_SAFE_STOCK: [StockPlugin; 23] = [
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
];

/// Built-in (non-stock) evaluated effects whose WGSL twin reproduces the
/// CPU kernel within u8 rounding (gentle point math, no time seeds, no
/// spatial resampling): the set the live GPU chain may take without
/// visual change. Same audit bar as [`CHAIN_SAFE_STOCK`].
pub const CHAIN_SAFE_BUILTIN: [&str; 14] = [
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect_filters as ef;

    #[test]
    fn uniforms_pack_roundtrip() {
        let u = FxUniforms::pack(&[1.0, 2.0, 3.0], 0.5, 1920.0, 1080.0, 7.0);
        assert_eq!(u.params[0], [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(u.misc, [0.5, 1920.0, 1080.0, 7.0]);
        assert_eq!(std::mem::size_of::<FxUniforms>(), 80);
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
        // Merge machinery both variants compile.
        FxMergePass::unsharp(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("unsharp compiles");
        FxMergePass::screen(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("screen compiles");
        FxPass::bright_extract(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("bright compiles");
        FxPass::blit(&gpu, wgpu::TextureFormat::Rgba8Unorm).expect("blit compiles");
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
        let uniforms = FxUniforms::pack(&[0.0, 50.0, 0.0, 0.0], 0.0, 4.0, 4.0, 0.0);
        pass.render(&gpu, src.view(), &dst, uniforms);
        let px = dst.read_texture_to_cpu(&gpu).expect("readback");
        assert_eq!(px.len(), 4 * 4 * 4);
        // Top rows cleared, bottom rows intact.
        assert_eq!(&px[0..8], &[0u8; 8], "row 0 must clear, got {:?}", &px[0..16]);
        assert_eq!(&px[48..64], &[255u8; 16], "row 3 must survive, got {:?}", &px[48..64]);
    }
}
