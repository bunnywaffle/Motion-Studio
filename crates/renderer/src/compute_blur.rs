//! High-performance GPU Gaussian blur using Compute Shaders (`@compute`)
//! and Workgroup Shared Memory (`var<workgroup>`).
//!
//! # Architecture:
//! - Separable 1D convolution: Horizontal compute pass followed by Vertical compute pass.
//! - Workgroup size: $16 \times 16 = 256$ threads (power-of-two warp/wavefront aligned).
//! - Shared Memory Apron: Cooperatively loads a tile of pixels including left/right or top/bottom
//!   apron into workgroup shared memory (`var<workgroup>`), calls `workgroupBarrier()`, and evaluates
//!   the filter purely out of on-chip L1/workgroup cache, cutting global VRAM bandwidth by up to $10\times$.

use crate::device::{GpuContext, GpuError, RenderTarget};
use bytemuck::{Pod, Zeroable};

/// Uniforms for the compute blur passes.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ComputeBlurUniforms {
    pub width: u32,
    pub height: u32,
    pub radius: f32,
    pub sigma: f32,
}

/// WGSL Compute Shader for 1D Separable Gaussian Blur with Workgroup Shared Memory.
pub const COMPUTE_BLUR_WGSL: &str = r#"
struct BlurParams {
    width: u32,
    height: u32,
    radius: f32,
    sigma: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var dst_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: BlurParams;

// Workgroup shared memory apron: 16x16 tile + 8px apron on each side (total width 32)
const TILE_DIM: u32 = 16u;
const APRON: u32 = 8u;
const SHARED_H_W: u32 = 32u; // 16 + 2 * 8
const SHARED_V_H: u32 = 32u; // 16 + 2 * 8

var<workgroup> tile_h: array<array<vec4<f32>, SHARED_H_W>, TILE_DIM>;
var<workgroup> tile_v: array<array<vec4<f32>, TILE_DIM>, SHARED_V_H>;

fn gauss_weight(i: f32, sigma: f32) -> f32 {
    let s = max(sigma, 0.5);
    return exp(-0.5 * (i * i) / (s * s));
}

// ---------------------------------------------------------------------------
// 1. Horizontal Pass
// ---------------------------------------------------------------------------
@compute @workgroup_size(16, 16)
fn cs_horizontal(
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) group_id: vec3<u32>,
    @builtin(global_invocation_id) global_id: vec3<u32>
) {
    let gx = i32(global_id.x);
    let gy = i32(global_id.y);
    let lx = local_id.x;
    let ly = local_id.y;
    let w = i32(params.width);
    let h = i32(params.height);

    // Center tile load
    let cx = clamp(gx, 0, w - 1);
    let cy = clamp(gy, 0, h - 1);
    tile_h[ly][lx + APRON] = textureLoad(src_tex, vec2<i32>(cx, cy), 0);

    // Cooperative halo/apron loading (left apron for lx < APRON, right apron for lx >= TILE_DIM - APRON)
    if (lx < APRON) {
        let left_gx = clamp(i32(group_id.x * TILE_DIM) + i32(lx) - i32(APRON), 0, w - 1);
        tile_h[ly][lx] = textureLoad(src_tex, vec2<i32>(left_gx, cy), 0);
    }
    if (lx >= TILE_DIM - APRON) {
        let right_gx = clamp(i32(group_id.x * TILE_DIM) + i32(lx) + i32(APRON), 0, w - 1);
        tile_h[ly][lx + 2u * APRON] = textureLoad(src_tex, vec2<i32>(right_gx, cy), 0);
    }

    workgroupBarrier();

    if (gx < w && gy < h) {
        let r = min(i32(ceil(params.radius)), i32(APRON));
        var acc = vec4<f32>(0.0);
        var wsum = 0.0;
        let s = max(params.sigma, 0.5);

        for (var i: i32 = -r; i <= r; i += 1) {
            let weight = gauss_weight(f32(i), s);
            let sample_color = tile_h[ly][i32(lx + APRON) + i];
            acc += sample_color * weight;
            wsum += weight;
        }

        let result = acc / max(wsum, 1e-5);
        textureStore(dst_tex, vec2<i32>(gx, gy), result);
    }
}

// ---------------------------------------------------------------------------
// 2. Vertical Pass
// ---------------------------------------------------------------------------
@compute @workgroup_size(16, 16)
fn cs_vertical(
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) group_id: vec3<u32>,
    @builtin(global_invocation_id) global_id: vec3<u32>
) {
    let gx = i32(global_id.x);
    let gy = i32(global_id.y);
    let lx = local_id.x;
    let ly = local_id.y;
    let w = i32(params.width);
    let h = i32(params.height);

    // Center tile load
    let cx = clamp(gx, 0, w - 1);
    let cy = clamp(gy, 0, h - 1);
    tile_v[ly + APRON][lx] = textureLoad(src_tex, vec2<i32>(cx, cy), 0);

    // Cooperative halo/apron loading (top apron for ly < APRON, bottom apron for ly >= TILE_DIM - APRON)
    if (ly < APRON) {
        let top_gy = clamp(i32(group_id.y * TILE_DIM) + i32(ly) - i32(APRON), 0, h - 1);
        tile_v[ly][lx] = textureLoad(src_tex, vec2<i32>(cx, top_gy), 0);
    }
    if (ly >= TILE_DIM - APRON) {
        let bot_gy = clamp(i32(group_id.y * TILE_DIM) + i32(ly) + i32(APRON), 0, h - 1);
        tile_v[ly + 2u * APRON][lx] = textureLoad(src_tex, vec2<i32>(cx, bot_gy), 0);
    }

    workgroupBarrier();

    if (gx < w && gy < h) {
        let r = min(i32(ceil(params.radius)), i32(APRON));
        var acc = vec4<f32>(0.0);
        var wsum = 0.0;
        let s = max(params.sigma, 0.5);

        for (var i: i32 = -r; i <= r; i += 1) {
            let weight = gauss_weight(f32(i), s);
            let sample_color = tile_v[i32(ly + APRON) + i][lx];
            acc += sample_color * weight;
            wsum += weight;
        }

        let result = acc / max(wsum, 1e-5);
        textureStore(dst_tex, vec2<i32>(gx, gy), result);
    }
}
"#;

/// Reusable compute blur pipeline.
pub struct ComputeBlurPipeline {
    horizontal_pipeline: wgpu::ComputePipeline,
    vertical_pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
}

impl ComputeBlurPipeline {
    /// Create a new compute blur pipeline on the device.
    pub fn new(gpu: &GpuContext) -> Result<Self, GpuError> {
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Compute Blur Shader"),
            source: wgpu::ShaderSource::Wgsl(COMPUTE_BLUR_WGSL.into()),
        });

        let bind_group_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Compute Blur Bind Group Layout"),
            entries: &[
                // Binding 0: src_tex (texture_2d<f32>)
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Binding 1: dst_tex (storage_2d<rgba8unorm, write>)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Binding 2: params (uniform)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Compute Blur Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let horizontal_pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Blur Horizontal Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("cs_horizontal"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let vertical_pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Blur Vertical Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("cs_vertical"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Compute Blur Uniform Buffer"),
            size: std::mem::size_of::<ComputeBlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            horizontal_pipeline,
            vertical_pipeline,
            bind_group_layout,
            uniform_buffer,
        })
    }

    /// Record a two-pass Gaussian blur into `encoder`.
    ///
    /// Reads from `src.view()` into `intermediate.view()` (horizontal pass),
    /// then from `intermediate.view()` into `dst.view()` (vertical pass).
    pub fn record_blur(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        src: &RenderTarget,
        intermediate: &RenderTarget,
        dst: &RenderTarget,
        radius_px: f32,
    ) {
        if radius_px <= 0.05 {
            return;
        }

        let width = src.width();
        let height = src.height();
        let sigma = (radius_px / 2.0).max(0.5);

        let uniforms = ComputeBlurUniforms {
            width,
            height,
            radius: radius_px,
            sigma,
        };
        gpu.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let h_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Blur Horizontal Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(intermediate.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let v_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Blur Vertical Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(intermediate.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(dst.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let workgroups_x = width.div_ceil(16);
        let workgroups_y = height.div_ceil(16);

        // Pass 1: Horizontal
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute Blur Horizontal Pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.horizontal_pipeline);
            cpass.set_bind_group(0, &h_bind_group, &[]);
            cpass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        // Pass 2: Vertical
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute Blur Vertical Pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.vertical_pipeline);
            cpass.set_bind_group(0, &v_bind_group, &[]);
            cpass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }
    }
}
