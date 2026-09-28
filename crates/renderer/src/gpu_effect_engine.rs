//! High-performance GPU effect engine coordinating Compute Blur, Fused Fragment passes,
//! and Double-Buffered transient texture pooling.
//!
//! # Architecture:
//! - **Compute Pipelines**: Multi-tap spatial filters (Gaussian blur) run as compute shaders
//!   with workgroup shared memory (`var<workgroup>`).
//! - **Fragment Pipelines**: Point and color operations (Brightness/Contrast, Tint, Invert,
//!   Levels, Vignette, Hue/Sat) are fused and run in hardware raster/fragment passes.
//! - **Transient Texture Pooling**: Reuses ping-pong render targets (`DoubleBufferedTarget`)
//!   to avoid runtime VRAM allocations during animation playback and scrubbing.
//! - **Single Command Buffer Submission**: Encodes all effect stages into a single
//!   `wgpu::CommandEncoder` and submits once per frame, preventing CPU-GPU synchronization stalls.

use crate::compute_blur::ComputeBlurPipeline;
use crate::device::{DoubleBufferedTarget, GpuContext, GpuError, RenderTarget};
use crate::fx_pass::{FxPass, FxUniforms};
use compositor::{EvaluatedEffect, EvaluatedEffectType};
use project::StockPlugin;

/// Unified GPU Effect Engine.
pub struct GpuEffectEngine {
    gpu: GpuContext,
    blur_pipeline: ComputeBlurPipeline,
    targets: Option<DoubleBufferedTarget>,
    intermediate_target: Option<RenderTarget>,
    fx_pass_cache: std::collections::HashMap<StockPlugin, FxPass>,
}

impl GpuEffectEngine {
    /// Create a new GPU effect engine with a headless or native device context.
    pub fn new(gpu: GpuContext) -> Result<Self, GpuError> {
        let blur_pipeline = ComputeBlurPipeline::new(&gpu)?;
        Ok(Self {
            gpu,
            blur_pipeline,
            targets: None,
            intermediate_target: None,
            fx_pass_cache: std::collections::HashMap::new(),
        })
    }

    /// Access the underlying GPU context.
    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// True when the engine runs on real hardware. Software-fallback
    /// adapters emulate compute on the CPU with submit + readback stalls
    /// that dwarf the native CPU kernel — callers must prefer the CPU
    /// path when this is false.
    pub fn is_hardware(&self) -> bool {
        !matches!(
            self.gpu.adapter_info().device_type,
            wgpu::DeviceType::Cpu
        )
    }

    /// Ensure render targets match the given dimensions.
    fn ensure_targets(&mut self, width: u32, height: u32) -> Result<(), GpuError> {
        let needs_new = match &self.targets {
            Some(t) => t.width() != width || t.height() != height,
            None => true,
        };
        if needs_new {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            self.targets = Some(DoubleBufferedTarget::with_format(&self.gpu, width, height, format)?);
            self.intermediate_target = Some(RenderTarget::with_format(&self.gpu, width, height, format)?);
        }
        Ok(())
    }

    /// Get or create an FxPass for a given stock plugin.
    fn get_or_create_fx_pass(&mut self, plugin: StockPlugin) -> Result<&FxPass, GpuError> {
        if !self.fx_pass_cache.contains_key(&plugin) {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let pass = FxPass::for_stock(&self.gpu, plugin, format)
                .map_err(GpuError::ShaderCompilation)?;
            self.fx_pass_cache.insert(plugin, pass);
        }
        Ok(&self.fx_pass_cache[&plugin])
    }

    /// Process an RGBA8 frame through the evaluated effect stack on the GPU.
    ///
    /// If `effects` has no enabled effects, returns `Ok(src_rgba.to_vec())` without GPU work.
    pub fn process_rgba_frame(
        &mut self,
        src_rgba: &[u8],
        width: u32,
        height: u32,
        effects: &[EvaluatedEffect],
        time_s: f32,
    ) -> Result<Vec<u8>, GpuError> {
        let active_effects: Vec<&EvaluatedEffect> = effects.iter().filter(|e| e.enabled).collect();
        if active_effects.is_empty() || width == 0 || height == 0 {
            return Ok(src_rgba.to_vec());
        }

        self.ensure_targets(width, height)?;

        // Pre-create any required stock passes before borrowing targets
        for eff in &active_effects {
            if let EvaluatedEffectType::Stock { plugin, .. } = &eff.effect_type {
                let _ = self.get_or_create_fx_pass(*plugin);
            }
        }

        let targets = self.targets.as_mut().expect("targets allocated");
        let intermediate = self.intermediate_target.as_ref().expect("intermediate allocated");

        // 1. Upload initial source pixels to read target
        targets.read_target().write_texture_rgba(&self.gpu, src_rgba);

        // 2. Encode all passes into a single CommandEncoder
        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GPU Effect Chain Encoder"),
        });

        for eff in active_effects {
            match &eff.effect_type {
                EvaluatedEffectType::GaussianBlur { radius } => {
                    if *radius > 0.05 {
                        self.blur_pipeline.record_blur(
                            &self.gpu,
                            &mut encoder,
                            targets.read_target(),
                            intermediate,
                            targets.write_target(),
                            *radius,
                        );
                        targets.swap();
                    }
                }
                EvaluatedEffectType::Stock { plugin, params, colors: _ } => {
                    if let Some(pass) = self.fx_pass_cache.get(plugin) {
                        let uniforms = FxUniforms::pack(params, time_s, width as f32, height as f32, 0.0);
                        pass.record_into(
                            &self.gpu,
                            &mut encoder,
                            targets.read_target().view(),
                            targets.write_target(),
                            uniforms,
                        );
                        targets.swap();
                    }
                }
                _ => {}
            }
        }

        // 3. Single command buffer submission
        self.gpu.queue.submit(std::iter::once(encoder.finish()));

        // 4. Download processed pixels back to CPU
        targets.read_active_to_cpu(&self.gpu)
    }

    /// Blur an RGBA8 buffer directly using the GPU compute blur pipeline with workgroup shared memory.
    pub fn blur_rgba(
        &mut self,
        src_rgba: &[u8],
        width: u32,
        height: u32,
        radius: f32,
    ) -> Result<Vec<u8>, GpuError> {
        let eff = EvaluatedEffect {
            id: "blur".to_string(),
            name: "Gaussian Blur".to_string(),
            enabled: true,
            effect_type: EvaluatedEffectType::GaussianBlur { radius },
        };
        self.process_rgba_frame(src_rgba, width, height, &[eff], 0.0)
    }
}
