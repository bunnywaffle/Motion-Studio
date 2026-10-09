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
use crate::fx_pass::{FxMergePass, FxPass, FxUniforms};
use compositor::{EvaluatedEffect, EvaluatedEffectType};
use project::StockPlugin;

/// Unified GPU Effect Engine.
pub struct GpuEffectEngine {
    gpu: GpuContext,
    blur_pipeline: ComputeBlurPipeline,
    targets: Option<DoubleBufferedTarget>,
    intermediate_target: Option<RenderTarget>,
    fx_pass_cache: std::collections::HashMap<StockPlugin, FxPass>,
    builtin_pass_cache: std::collections::HashMap<&'static str, FxPass>,
    merge_pass_cache: std::collections::HashMap<&'static str, FxMergePass>,
    bright_pass: Option<FxPass>,
    blit_pass: Option<FxPass>,
    /// Scratch target for multi-stage spatial effects (sharpen blur source,
    /// bloom blurred glow): same size as the ping-pong pair, pooled.
    scratch: Option<RenderTarget>,
    /// Pooled readback staging buffer (avoids a GPU buffer alloc + destroy
    /// on every blur/readback call; recreated only when dims change).
    readback: Option<PooledReadback>,
}

/// Reusable MAP_READ staging buffer for texture readbacks.
struct PooledReadback {
    buf: wgpu::Buffer,
    /// Padded bytes-per-row the buffer was created for.
    padded_bpr: u32,
    /// Height (rows) the buffer was created for.
    height: u32,
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
            builtin_pass_cache: std::collections::HashMap::new(),
            merge_pass_cache: std::collections::HashMap::new(),
            bright_pass: None,
            blit_pass: None,
            scratch: None,
            readback: None,
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
            self.scratch = Some(RenderTarget::with_format(&self.gpu, width, height, format)?);
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

    /// Get or create an FxPass for a built-in (non-stock) effect id.
    fn get_or_create_builtin_pass(&mut self, id: &'static str) -> Result<&FxPass, GpuError> {
        if !self.builtin_pass_cache.contains_key(id) {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let pass = FxPass::for_builtin(&self.gpu, id, format)
                .map_err(GpuError::ShaderCompilation)?;
            self.builtin_pass_cache.insert(id, pass);
        }
        Ok(&self.builtin_pass_cache[id])
    }

    /// Get or create a two-texture merge pass ("unsharp" / "screen").
    fn get_or_create_merge_pass(&mut self, id: &'static str) -> Result<&FxMergePass, GpuError> {
        if !self.merge_pass_cache.contains_key(id) {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let pass = match id {
                "screen" => FxMergePass::screen(&self.gpu, format),
                _ => FxMergePass::unsharp(&self.gpu, format),
            }
            .map_err(GpuError::ShaderCompilation)?;
            self.merge_pass_cache.insert(id, pass);
        }
        Ok(&self.merge_pass_cache[id])
    }

    /// Get or create the bright-pass extract stage.
    fn get_or_create_bright_pass(&mut self) -> Result<&FxPass, GpuError> {
        if self.bright_pass.is_none() {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            self.bright_pass =
                Some(FxPass::bright_extract(&self.gpu, format).map_err(GpuError::ShaderCompilation)?);
        }
        Ok(self.bright_pass.as_ref().expect("just created"))
    }

    /// Get or create the identity blit stage.
    fn get_or_create_blit_pass(&mut self) -> Result<&FxPass, GpuError> {
        if self.blit_pass.is_none() {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            self.blit_pass =
                Some(FxPass::blit(&self.gpu, format).map_err(GpuError::ShaderCompilation)?);
        }
        Ok(self.blit_pass.as_ref().expect("just created"))
    }

    /// Read a render target back reusing the pooled staging buffer
    /// (recreated only when dims change): steady frames perform zero GPU
    /// buffer allocations on readback.
    fn read_pooled(
        pool: &mut Option<PooledReadback>,
        gpu: &GpuContext,
        target: &RenderTarget,
    ) -> Result<Vec<u8>, GpuError> {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded = target.width() * 4;
        let padded = unpadded.div_ceil(align) * align;
        let height = target.height();
        let need = (padded * height) as u64;
        let reuse = matches!(pool, Some(r) if r.padded_bpr == padded && r.height == height);
        if !reuse {
            *pool = Some(PooledReadback {
                buf: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Pooled Readback Staging Buffer"),
                    size: need,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                padded_bpr: padded,
                height,
            });
        }
        let staging = &pool.as_ref().expect("pool just created").buf;
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pooled Readback Encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: target.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width: target.width(),
                height,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit(std::iter::once(encoder.finish()));
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        gpu.device.poll(wgpu::Maintain::Wait);
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(GpuError::BufferAsyncError(format!("{e:?}"))),
            Err(e) => return Err(GpuError::BufferAsyncError(e.to_string())),
        }
        let mapped_view = slice.get_mapped_range();
        let mut tightly_packed = Vec::with_capacity((target.width() * height * 4) as usize);
        for row in 0..height {
            let start = (row * padded) as usize;
            let end = start + unpadded as usize;
            tightly_packed.extend_from_slice(&mapped_view[start..end]);
        }
        drop(mapped_view);
        staging.unmap();
        Ok(tightly_packed)
    }

    /// True when every enabled effect in the chain has a native GPU pass,
    /// so `process_rgba_frame` reproduces the CPU chain instead of
    /// silently skipping stages. Membership is the parity-audited
    /// [`crate::fx_pass::CHAIN_SAFE_STOCK`] set for stock plug-ins and
    /// [`crate::fx_pass::CHAIN_SAFE_BUILTIN`] for built-in color ops;
    /// compilability probes the same `for_stock` / `for_builtin`
    /// constructors the chain itself uses. GaussianBlur is deliberately
    /// excluded: blur runs in its own raster stage via `blur_buffer`,
    /// which already picks the GPU compute path when hardware exists.
    pub fn supports_fx_chain(&mut self, effects: &[compositor::EvaluatedEffect]) -> bool {
        let mut any = false;
        // Posterize-family after an edge map speckles: edge output sits on
        // posterize boundaries, and u8 inter-pass rounding flips whole
        // bands (empirically 0.3 on edge->cartoon). Decline those stacks
        // to the exact CPU path.
        let mut saw_edge = false;
        for eff in effects {
            if !eff.enabled {
                continue;
            }
            match &eff.effect_type {
                compositor::EvaluatedEffectType::Stock { plugin, .. } => {
                    if matches!(
                        plugin,
                        StockPlugin::EdgeDetect
                    ) {
                        saw_edge = true;
                    } else if saw_edge
                        && matches!(
                            plugin,
                            StockPlugin::Cartoon | StockPlugin::Posterize | StockPlugin::Mosaic
                        )
                    {
                        return false;
                    }
                    if !crate::fx_pass::CHAIN_SAFE_STOCK.contains(plugin) {
                        return false;
                    }
                    if self.get_or_create_fx_pass(*plugin).is_err() {
                        return false;
                    }
                }
                eff_type => {
                    // Unsharp mask needs the blurred buffer beside the
                    // original: blur into scratch, then merge. A zero
                    // amount is a true no-op on both paths: skip it.
                    // Wide radii exceed the compute apron (blur_buffer
                    // downsamples there): keep those on the CPU kernel.
                    if let compositor::EvaluatedEffectType::Sharpen { amount, radius } = eff_type {
                        let k = (*amount / 100.0).clamp(0.0, 2.0);
                        if k <= 0.01 {
                            continue;
                        }
                        if *radius > 8.0 {
                            return false;
                        }
                        if self.get_or_create_merge_pass("unsharp").is_err() {
                            return false;
                        }
                        any = true;
                        continue;
                    }
                    let Some(id) = crate::fx_pass::builtin_gpu_id(eff_type) else {
                        return false;
                    };
                    if !crate::fx_pass::CHAIN_SAFE_BUILTIN.contains(&id) {
                        return false;
                    }
                    // Degenerate levels spans step per channel, which
                    // amplifies u8 inter-pass rounding into full 0/1 flips:
                    // keep those on the exact CPU path.
                    if let compositor::EvaluatedEffectType::Levels { input_black, input_white, .. } = eff_type {
                        let ib = (input_black / 255.0).clamp(0.0, 1.0);
                        let iw = (input_white / 255.0).clamp(0.0, 1.0);
                        if (iw - ib).abs() < 1e-5 {
                            return false;
                        }
                    }
                    // Displacement offsets derive from source luminance, so u8
                    // source quantization (≈0.002) becomes a position error
                    // that hard edges amplify into visible flips: keep large
                    // offsets on the exact CPU path (empirically ≈0.0012
                    // error per px of offset on hard-edge content).
                    if let compositor::EvaluatedEffectType::DisplacementMap { max_horizontal, max_vertical, source_mode, channel_h, channel_v, map_scale, wrap, .. } = eff_type {
                        if max_horizontal.abs() > 8.0 || max_vertical.abs() > 8.0 {
                            return false;
                        }
                        // The WGSL twin is legacy self-luminance only; any
                        // new knob (noise map, channel select, scale, wrap)
                        // stays on the exact CPU path (honest badge).
                        if source_mode.round() != 0.0
                            || channel_h.round() != 4.0
                            || channel_v.round() != 4.0
                            || (map_scale - 1.0).abs() > 1e-4
                            || wrap.round() != 0.0
                        {
                            return false;
                        }
                    }
                    // The warp twin covers the sine-wave part only; pinned
                    // warps take the CPU mesh path.
                    if let compositor::EvaluatedEffectType::Warp { pins, .. } = eff_type {
                        if pins.iter().any(|p| !p.is_identity()) {
                            return false;
                        }
                    }
                    if self.get_or_create_builtin_pass(id).is_err() {
                        return false;
                    }
                }
            }
            any = true;
        }
        any
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

        // Pre-create any required passes before borrowing targets
        for eff in &active_effects {
            if let EvaluatedEffectType::Stock { plugin, .. } = &eff.effect_type {
                let _ = self.get_or_create_fx_pass(*plugin);
            } else if matches!(&eff.effect_type, EvaluatedEffectType::Sharpen { .. }) {
                let _ = self.get_or_create_merge_pass("unsharp");
            } else if let Some(id) = crate::fx_pass::builtin_gpu_id(&eff.effect_type) {
                let _ = self.get_or_create_builtin_pass(id);
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
                    // Card 3D packs the shared-plan inverse homography
                    // (same matrix the CPU kernel solves): identity skips,
                    // culled/degenerate bail to the exact CPU path via Err.
                    if *plugin == StockPlugin::Card3d {
                        use compositor::fx::{card_3d_plan, Card3dPlan};
                        match card_3d_plan(params, width as f32, height as f32) {
                            Card3dPlan::Identity => continue,
                            Card3dPlan::Clear | Card3dPlan::Keep => {
                                return Err(GpuError::Declined(
                                    "card 3d culled or degenerate".to_string(),
                                ));
                            }
                            Card3dPlan::Project(inv) => {
                                if let Some(pass) = self.fx_pass_cache.get(plugin) {
                                    // Row-padded so the twin reads matrix
                                    // rows as params[0..2].xyz.
                                    let m = [
                                        inv[0], inv[1], inv[2], 0.0,
                                        inv[3], inv[4], inv[5], 0.0,
                                        inv[6], inv[7], inv[8], 0.0,
                                        0.0, 0.0, 0.0, 0.0,
                                    ];
                                    let uniforms = FxUniforms::pack(&m, time_s, width as f32, height as f32, 0.0);
                                    pass.record_into(
                                        &self.gpu,
                                        &mut encoder,
                                        targets.read_target().view(),
                                        targets.write_target(),
                                        uniforms,
                                    );
                                    targets.swap();
                                }
                                continue;
                            }
                        }
                    }
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
                eff_type => {
                    // Unsharp mask: compute-blur the chain cursor into
                    // scratch, then merge (orig, blurred) into write.
                    if let EvaluatedEffectType::Sharpen { amount, radius } = eff_type {
                        let k = (*amount / 100.0).clamp(0.0, 2.0);
                        if k > 0.01 {
                            if let (Some(merge), Some(scratch)) = (
                                self.merge_pass_cache.get("unsharp"),
                                self.scratch.as_ref(),
                            ) {
                                let uniforms = FxUniforms::pack(&[k], time_s, width as f32, height as f32, 0.0);
                                self.blur_pipeline.record_blur(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target(),
                                    intermediate,
                                    scratch,
                                    radius.max(0.5),
                                );
                                merge.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target().view(),
                                    scratch.view(),
                                    targets.write_target(),
                                    uniforms,
                                );
                                targets.swap();
                            }
                        }
                        continue;
                    }
                    // Built-in color ops: pack evaluated fields exactly as
                    // the WGSL twin's call site expects (see
                    // `FxPass::for_builtin`). Ten slots: two vec4s hold a
                    // full RGB pair, so `params[1].xyz` stays addressable.
                    let (id, params): (&'static str, [f32; 10]) = match eff_type {
                        EvaluatedEffectType::BrightnessContrast { brightness, contrast } => {
                            ("brightness_contrast", [*brightness, *contrast, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::Tint { map_black, map_white, amount } => (
                            "tint",
                            [*amount, map_black.r, map_black.g, map_black.b, map_white.r, map_white.g, map_white.b, 0.0, 0.0, 0.0],
                        ),
                        EvaluatedEffectType::Levels { input_black, input_white, gamma, output_black, output_white } => (
                            "levels",
                            [*input_black, *input_white, *gamma, *output_black, *output_white, 0.0, 0.0, 0.0, 0.0, 0.0],
                        ),
                        EvaluatedEffectType::HueSaturation { hue_shift, saturation, lightness } => (
                            "hue_saturation",
                            [*hue_shift, *saturation, *lightness, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                        ),
                        EvaluatedEffectType::Invert { amount } => {
                            ("invert", [*amount, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::Exposure { exposure } => {
                            ("exposure", [*exposure, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::Vibrance { vibrance } => {
                            ("vibrance", [*vibrance, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::ChromaKey { key_color, tolerance, feather } => (
                            "chroma_key",
                            [key_color.r, key_color.g, key_color.b, *tolerance, *feather, 0.0, 0.0, 0.0, 0.0, 0.0],
                        ),
                        EvaluatedEffectType::LumaKey { threshold, feather } => {
                            ("luma_key", [*threshold, *feather, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::SwapColor { from_color, to_color, tolerance, feather } => (
                            "swap_color",
                            [from_color.r, from_color.g, from_color.b, 0.0, to_color.r, to_color.g, to_color.b, 0.0, *tolerance, *feather],
                        ),
                        EvaluatedEffectType::Vignette { amount, softness } => {
                            ("vignette", [*amount, *softness, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::DisplacementMap { max_horizontal, max_vertical, .. } => {
                            ("displacement", [*max_horizontal, *max_vertical, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::Warp { amount, scale, .. } => {
                            ("warp", [*amount, *scale, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::CelShading { levels, edge } => {
                            ("cel_shading", [*levels, *edge, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::OilPaint { radius, amount } => {
                            ("oil_paint", [*radius, *amount, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        _ => continue,
                    };
                    if let Some(pass) = self.builtin_pass_cache.get(id) {
                        let uniforms = FxUniforms::pack(&params, time_s, width as f32, height as f32, 0.0);
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
            }
        }

        // 3. Single command buffer submission
        self.gpu.queue.submit(std::iter::once(encoder.finish()));

        // 4. Download processed pixels back to CPU (pooled staging buffer:
        // steady frames allocate nothing here).
        let gpu = &self.gpu;
        let pool = &mut self.readback;
        let read = targets.read_target();
        Self::read_pooled(pool, gpu, read)
    }

    /// Full spatial bloom (bright-pass -> separable blur -> screen merge),
    /// mirroring `apply_bloom` stage for stage. Hardware-only by
    /// construction (callers gate on the hardware engine, so the blur is
    /// the same pipeline `blur_buffer` takes).
    pub fn bloom_rgba(
        &mut self,
        src_rgba: &[u8],
        width: u32,
        height: u32,
        intensity: f32,
        radius_px: f32,
    ) -> Result<Vec<u8>, GpuError> {
        let k = (intensity / 100.0).clamp(0.0, 1.0);
        if width == 0 || height == 0 {
            return Ok(src_rgba.to_vec());
        }
        self.ensure_targets(width, height)?;
        self.get_or_create_bright_pass()?;
        self.get_or_create_merge_pass("screen")?;
        self.get_or_create_blit_pass()?;

        let targets = self.targets.as_mut().expect("targets allocated");
        let intermediate = self.intermediate_target.as_ref().expect("intermediate allocated");
        let scratch = self.scratch.as_ref().expect("scratch allocated");
        let bright = self.bright_pass.as_ref().expect("bright allocated");
        let screen = self.merge_pass_cache.get("screen").expect("screen allocated");
        let blit = self.blit_pass.as_ref().expect("blit allocated");

        // 1. Upload source to the chain cursor.
        targets.read_target().write_texture_rgba(&self.gpu, src_rgba);

        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GPU Bloom Encoder"),
        });
        let uniforms = FxUniforms::pack(&[k], 0.0, width as f32, height as f32, 0.0);
        // 2. Bright-pass into write.
        bright.record_into(
            &self.gpu,
            &mut encoder,
            targets.read_target().view(),
            targets.write_target(),
            FxUniforms::pack(&[], 0.0, width as f32, height as f32, 0.0),
        );
        // 3. Separable blur of the bright buffer into scratch.
        self.blur_pipeline.record_blur(
            &self.gpu,
            &mut encoder,
            targets.write_target(),
            intermediate,
            scratch,
            radius_px.max(1.0),
        );
        // 4. Screen (orig, glow) into intermediate.
        screen.record_into(
            &self.gpu,
            &mut encoder,
            targets.read_target().view(),
            scratch.view(),
            intermediate,
            uniforms,
        );
        // 5. Blit back into the cursor for readback.
        blit.record_into(
            &self.gpu,
            &mut encoder,
            intermediate.view(),
            targets.read_target(),
            FxUniforms::pack(&[], 0.0, width as f32, height as f32, 0.0),
        );
        self.gpu.queue.submit(std::iter::once(encoder.finish()));

        let gpu = &self.gpu;
        let pool = &mut self.readback;
        let read = targets.read_target();
        Self::read_pooled(pool, gpu, read)
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
