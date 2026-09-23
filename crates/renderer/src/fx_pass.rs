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

    /// Run the pass: `source_view` -> `target` with packed uniforms.
    pub fn render(
        &self,
        gpu: &GpuContext,
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
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Fx Command Encoder"),
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
        gpu.queue.submit(std::iter::once(encoder.finish()));
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
        // Slug of the plug-in id must appear in one of the registries.
        for plugin in stock_wgsl_plugins() {
            let slug = plugin.plugin_id().rsplit('.').next().unwrap();
            let want = format!("stock_{slug}");
            assert!(
                color_ids.contains(&want.as_str()) || uv_ids.contains(&want.as_str()),
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
}
