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

use crate::compute_blur::{ComputeBlurPipeline, ComputeBoxPipeline};
use crate::device::{DoubleBufferedTarget, GpuContext, GpuError, RenderTarget};
use crate::fx_pass::{FxMergePass, FxPass, FxUniforms};
use compositor::{EvaluatedEffect, EvaluatedEffectType};
use project::StockPlugin;

thread_local! {
    /// Viewport-side gate mirrored from the rasterizer (per thread, so
    /// concurrent per-layer div renders never cross-talk): exotic blend
    /// modes composite the aura through their own math and keep it on the
    /// exact CPU blit path; the export path never enables the chain at all.
    static AURA_GPU_OK: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// Whether the last chain pass served the Drop Shadow / Outer Glow
    /// aura stages on the GPU â€” the rasterizer's CPU blit stage checks
    /// it right after and skips its own aura pass to avoid double-applying.
    static AURA_GPU_HANDLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Flag whether the current layer render may route the built-in aura
/// (Drop Shadow / Outer Glow) through the GPU chain. Called on the
/// rasterizing thread before `gpu_fx_chain` (see `buffer.rs`).
pub fn set_aura_gpu_ok(ok: bool) {
    AURA_GPU_OK.with(|c| c.set(ok));
}
pub(crate) fn aura_gpu_ok() -> bool {
    AURA_GPU_OK.with(|c| c.get())
}
/// Clear the per-call handled marker (must be called at the entry of every
/// chain attempt so a stale flag never skips a following CPU aura pass).
pub fn reset_aura_gpu_handled() {
    AURA_GPU_HANDLED.with(|c| c.set(false));
}
pub fn aura_gpu_handled() -> bool {
    AURA_GPU_HANDLED.with(|c| c.get())
}
fn set_aura_gpu_handled() {
    AURA_GPU_HANDLED.with(|c| c.set(true));
}

/// Unified GPU Effect Engine.
pub struct GpuEffectEngine {
    gpu: GpuContext,
    blur_pipeline: ComputeBlurPipeline,
    box_pipeline: ComputeBoxPipeline,
    targets: Option<DoubleBufferedTarget>,
    intermediate_target: Option<RenderTarget>,
    fx_pass_cache: std::collections::HashMap<StockPlugin, FxPass>,
    builtin_pass_cache: std::collections::HashMap<&'static str, FxPass>,
    merge_pass_cache: std::collections::HashMap<&'static str, FxMergePass>,
    bright_pass: Option<FxPass>,
    blit_pass: Option<FxPass>,
    /// Long-shadow march stage pass for the multi-stage stock path
    /// (march â†’ blur â†’ under-merge).
    march_pass: Option<FxPass>,
    /// Aura fill stages for the built-in Drop Shadow / Outer Glow path
    /// (colorize â†’ compute blur â†’ under-merge).
    glow_fill: Option<FxPass>,
    shadow_fill: Option<FxPass>,
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

/// Resolve a stock plug-in's RGBA color slots for uniform packing:
/// evaluated colors win, descriptor defaults cover old project files
/// with shorter vectors (same fallback as the CPU kernels).
fn stock_uniform_colors(plugin: StockPlugin, colors: &[project::Color]) -> [[f32; 4]; 4] {
    let slots = project::stock_color_slots(plugin);
    let mut out = [[0.0f32; 4]; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        let c = colors
            .get(i)
            .copied()
            .unwrap_or_else(|| {
                slots
                    .get(i)
                    .map(|s| project::stock_default_color(plugin, s))
                    .unwrap_or(project::Color::WHITE)
            });
        *slot = [c.r, c.g, c.b, c.a];
    }
    out
}

impl GpuEffectEngine {
    /// Create a new GPU effect engine with a headless or native device context.
    pub fn new(gpu: GpuContext) -> Result<Self, GpuError> {
        let blur_pipeline = ComputeBlurPipeline::new(&gpu)?;
        let box_pipeline = ComputeBoxPipeline::new(&gpu)?;
        Ok(Self {
            gpu,
            blur_pipeline,
            box_pipeline,
            targets: None,
            intermediate_target: None,
            fx_pass_cache: std::collections::HashMap::new(),
            builtin_pass_cache: std::collections::HashMap::new(),
            merge_pass_cache: std::collections::HashMap::new(),
            bright_pass: None,
            blit_pass: None,
            march_pass: None,
            glow_fill: None,
            shadow_fill: None,
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
    /// that dwarf the native CPU kernel â€” callers must prefer the CPU
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

    /// Get or create a two-texture merge pass ("unsharp" / "screen" /
    /// "shadow_under"; Glow reuses "screen" since its blurred layer holds
    /// straight bytes exactly like the bloom stage).
    fn get_or_create_merge_pass(&mut self, id: &'static str) -> Result<&FxMergePass, GpuError> {
        if !self.merge_pass_cache.contains_key(id) {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let pass = match id {
                "screen" => FxMergePass::screen(&self.gpu, format),
                "shadow_under" => FxMergePass::shadow_under(&self.gpu, format),
                "under_prem" => FxMergePass::under_prem(&self.gpu, format),
                _ => FxMergePass::unsharp(&self.gpu, format),
            }
            .map_err(GpuError::ShaderCompilation)?;
            self.merge_pass_cache.insert(id, pass);
        }
        Ok(&self.merge_pass_cache[id])
    }

    /// Get or create the long-shadow march stage.
    fn get_or_create_march_pass(&mut self) -> Result<&FxPass, GpuError> {
        if self.march_pass.is_none() {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            self.march_pass =
                Some(FxPass::shadow_march(&self.gpu, format).map_err(GpuError::ShaderCompilation)?);
        }
        Ok(self.march_pass.as_ref().expect("just created"))
    }

    /// Get or create the built-in aura fill stages (both families compile
    /// lazily in one go; the merge + blit pass ride the existing caches).
    fn get_or_create_aura_fill_passes(&mut self) -> Result<(), GpuError> {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        if self.glow_fill.is_none() {
            self.glow_fill =
                Some(FxPass::glow_fill(&self.gpu, format).map_err(GpuError::ShaderCompilation)?);
        }
        if self.shadow_fill.is_none() {
            self.shadow_fill =
                Some(FxPass::shadow_fill(&self.gpu, format).map_err(GpuError::ShaderCompilation)?);
        }
        self.get_or_create_merge_pass("under_prem")?;
        self.get_or_create_blit_pass()?;
        Ok(())
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
        // Two auras ride the CPU blit stage in authored order (shadow
        // under glow); a GPU twin serving both below one content merge is
        // out of scope, so multi-aura stacks keep the CPU path.
        let aura_count = effects
            .iter()
            .filter(|e| {
                e.enabled
                    && matches!(
                        e.effect_type,
                        compositor::EvaluatedEffectType::DropShadow { .. }
                            | compositor::EvaluatedEffectType::OuterGlow { .. }
                    )
            })
            .count();
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
                compositor::EvaluatedEffectType::Stock { plugin, params, .. } => {
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
                    // Multi-stage Glow stays CPU (see CHAIN_SAFE_STOCK):
                    // threshold presence flips across the u8 boundary
                    // while the white screen amplifies any nonzero mask
                    // to full strength â€” unboundable in a u8 chain.
                    if *plugin == StockPlugin::Glow {
                        return false;
                    }
                    // Light Rays screen-blend mode screens with the tint
                    // wherever the march is merely nonzero: a single tap
                    // straddling the smoothstep floor flips presence, and
                    // the screen amplifies that LSB into a full lift.
                    // Additive mode stays magnitude-proportional and safe.
                    if *plugin == StockPlugin::LightRays {
                        use compositor::fx::stock_p;
                        if stock_p(*plugin, params, 10).round() >= 1.0 {
                            return false;
                        }
                    }
                    // (Lens Flare streaks need no gate: the 48-tap streak
                    // footprint makes exact-zero straddles uninhabitable
                    // on real gradients â€” the parity test guards this.)
                    if *plugin == StockPlugin::LongShadow {
                        use compositor::fx::stock_p;
                        if stock_p(*plugin, params, 5) > 8.0 {
                            return false;
                        }
                        if self.get_or_create_march_pass().is_err()
                            || self.get_or_create_merge_pass("shadow_under").is_err()
                            || self.get_or_create_blit_pass().is_err()
                        {
                            return false;
                        }
                        any = true;
                        continue;
                    }
                    if *plugin == StockPlugin::BoxBlur || *plugin == StockPlugin::Defocus {
                        use compositor::fx::stock_p;
                        let noop = if *plugin == StockPlugin::Defocus { 0.5 } else { 1.0 };
                        let r = if *plugin == StockPlugin::Defocus {
                            stock_p(*plugin, params, 0).clamp(0.0, 40.0)
                        } else {
                            stock_p(*plugin, params, 0).clamp(0.0, 64.0)
                        };
                        if r < noop {
                            continue;
                        }
                        any = true;
                        continue;
                    }
                    // Bokeh stays on the exact CPU path: the blurred-highlight
                    // alpha tail below 1/255 carries full-strength screen
                    // presence on the CPU (`screen_over` unpremultiplies)
                    // but quantizes to zero in any u8 chain â€” an unboundable
                    // halo fringe (measured 0.88 on gradients).
                    if *plugin == StockPlugin::Bokeh {
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
                    // Built-in aura (Drop Shadow / Outer Glow): colorize â†’
                    // compute blur â†’ under-merge. Radii cap at the compute
                    // blur's shared-memory apron (radii beyond that keep
                    // the CPU kernel, which downsamples), and the caller
                    // flags whether the aura is GPU-safe at all (viewport
                    // Normal blend; exotic modes composite the aura
                    // through their own math and stay CPU).
                    if let compositor::EvaluatedEffectType::DropShadow {
                        softness,
                        opacity,
                        ..
                    } = eff_type
                    {
                        if aura_count > 1 || !aura_gpu_ok() || *softness > 8.0 {
                            return false;
                        }
                        // CPU blit renders the shadow for any distance
                        // (softness halo spreads past the content even at
                        // distance 0) â€” only degenerate strength skips.
                        if (opacity / 100.0).clamp(0.0, 1.0) * 0.75 <= 0.005 {
                            continue;
                        }
                        if self.get_or_create_aura_fill_passes().is_err() {
                            return false;
                        }
                        any = true;
                        continue;
                    }
                    if let compositor::EvaluatedEffectType::OuterGlow {
                        size, opacity, ..
                    } = eff_type
                    {
                        if aura_count > 1 || !aura_gpu_ok() || *size > 8.0 {
                            return false;
                        }
                        // CPU blit stage skips glow at opacity <= 1% and
                        // radius < 0.5 px â€” mirror exactly.
                        if (opacity / 100.0).clamp(0.0, 1.0) <= 0.01 || *size <= 0.5 {
                            continue;
                        }
                        if self.get_or_create_aura_fill_passes().is_err() {
                            return false;
                        }
                        any = true;
                        continue;
                    }
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
                    // source quantization (â‰ˆ0.002) becomes a position error
                    // that hard edges amplify into visible flips: keep large
                    // offsets on the exact CPU path (empirically â‰ˆ0.0012
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
                if *plugin == StockPlugin::LongShadow {
                    let _ = self.get_or_create_march_pass();
                    let _ = self.get_or_create_merge_pass("shadow_under");
                    let _ = self.get_or_create_blit_pass();
                    continue;
                }
                let _ = self.get_or_create_fx_pass(*plugin);
            } else if matches!(&eff.effect_type, EvaluatedEffectType::Sharpen { .. }) {
                let _ = self.get_or_create_merge_pass("unsharp");
            } else if let Some(id) = crate::fx_pass::builtin_gpu_id(&eff.effect_type) {
                let _ = self.get_or_create_builtin_pass(id);
            } else if matches!(
                &eff.effect_type,
                EvaluatedEffectType::DropShadow { .. } | EvaluatedEffectType::OuterGlow { .. }
            ) {
                let _ = self.get_or_create_aura_fill_passes();
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
                EvaluatedEffectType::Stock { plugin, params, colors } => {
                    // Box blur / defocus: separable box compute passes
                    // (`box_blur` exact, defocus = box(r) then box(r*0.7)).
                    if *plugin == StockPlugin::BoxBlur || *plugin == StockPlugin::Defocus {
                        use compositor::fx::stock_p;
                        let scratch = self.intermediate_target.as_ref().expect("intermediate allocated");
                        if *plugin == StockPlugin::BoxBlur {
                            if self.box_pipeline.record_box(
                                &self.gpu,
                                &mut encoder,
                                targets.read_target(),
                                scratch,
                                targets.write_target(),
                                stock_p(*plugin, params, 0),
                            ) {
                                targets.swap();
                            }
                        } else {
                            let r = stock_p(*plugin, params, 0).clamp(0.0, 40.0);
                            if r >= 0.5
                                && self.box_pipeline.record_box(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target(),
                                    scratch,
                                    targets.write_target(),
                                    r,
                                )
                            {
                                targets.swap();
                                if self.box_pipeline.record_box(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target(),
                                    scratch,
                                    targets.write_target(),
                                    r * 0.7,
                                ) {
                                    targets.swap();
                                }
                            }
                        }
                        continue;
                    }
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
                                    let uniforms = FxUniforms::pack(&m, &stock_uniform_colors(*plugin, colors), time_s, width as f32, height as f32, 0.0);
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
                    // Corner Pin shares the Card3D project body through its
                    // own shared plan (same solve the CPU kernel runs):
                    // identity skips, degenerate bails to CPU via Err.
                    if *plugin == StockPlugin::CornerPin {
                        use compositor::fx::{corner_pin_plan, Card3dPlan};
                        match corner_pin_plan(params) {
                            Card3dPlan::Identity => continue,
                            Card3dPlan::Clear | Card3dPlan::Keep => {
                                return Err(GpuError::Declined(
                                    "corner pin degenerate".to_string(),
                                ));
                            }
                            Card3dPlan::Project(inv) => {
                                if let Some(pass) = self.fx_pass_cache.get(plugin) {
                                    let m = [
                                        inv[0], inv[1], inv[2], 0.0,
                                        inv[3], inv[4], inv[5], 0.0,
                                        inv[6], inv[7], inv[8], 0.0,
                                        0.0, 0.0, 0.0, 0.0,
                                    ];
                                    let uniforms = FxUniforms::pack(&m, &stock_uniform_colors(*plugin, colors), time_s, width as f32, height as f32, 0.0);
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
                    // Stock Long Shadow: march â†’ gaussian blur â†’ under
                    // merge (same stage shape; degenerate params skip
                    // exactly like the CPU early-out, in every mode).
                    if *plugin == StockPlugin::LongShadow {
                        use compositor::fx::stock_p;
                        let dist = stock_p(*plugin, params, 1).clamp(0.0, 1024.0);
                        let strength = (stock_p(*plugin, params, 8) / 100.0).clamp(0.0, 1.0);
                        if dist < 0.5 || strength <= 0.001 {
                            continue;
                        }
                        let soft = stock_p(*plugin, params, 5).clamp(0.0, 60.0);
                        let mode = stock_p(*plugin, params, 7).round().clamp(0.0, 2.0);
                        let scratch = self.scratch.as_ref().expect("targets allocated");
                        if let Some(march) = self.march_pass.as_ref() {
                            let full = FxUniforms::pack(
                                params,
                                &stock_uniform_colors(*plugin, colors),
                                time_s,
                                width as f32,
                                height as f32,
                                0.0,
                            );
                            march.record_into(
                                &self.gpu,
                                &mut encoder,
                                targets.read_target().view(),
                                scratch,
                                full,
                            );
                        }
                        if soft > 0.5 {
                            self.blur_pipeline.record_blur(
                                &self.gpu,
                                &mut encoder,
                                scratch,
                                intermediate,
                                targets.write_target(),
                                soft,
                            );
                        } else if let Some(blit) = self.blit_pass.as_ref() {
                            blit.record_into(
                                &self.gpu,
                                &mut encoder,
                                scratch.view(),
                                targets.write_target(),
                                FxUniforms::pack(&[], &[], time_s, width as f32, height as f32, 0.0),
                            );
                        }
                        if let Some(merge) = self.merge_pass_cache.get("shadow_under") {
                            let uniforms = FxUniforms::pack(
                                &[mode],
                                &[],
                                time_s,
                                width as f32,
                                height as f32,
                                0.0,
                            );
                            merge.record_into(
                                &self.gpu,
                                &mut encoder,
                                targets.read_target().view(),
                                targets.write_target().view(),
                                intermediate,
                                uniforms,
                            );
                        }
                        if let Some(blit) = self.blit_pass.as_ref() {
                            blit.record_into(
                                &self.gpu,
                                &mut encoder,
                                intermediate.view(),
                                targets.read_target(),
                                FxUniforms::pack(&[], &[], time_s, width as f32, height as f32, 0.0),
                            );
                        }
                        continue;
                    }
                    if let Some(pass) = self.fx_pass_cache.get(plugin) {
                        // Resolve against descriptor defaults so old saved
                        // projects (shorter param vecs) match the CPU path,
                        // which reads the same defaults via stock_p.
                        let resolved = compositor::fx::stock_params_resolved(*plugin, params);
                        let uniforms = FxUniforms::pack(&resolved, &stock_uniform_colors(*plugin, colors), time_s, width as f32, height as f32, 0.0);
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
                    // Built-in aura (Drop Shadow / Outer Glow):
                    // colorize -> compute blur -> under-merge, then land
                    // the result back in the chain cursor. Exactly the CPU
                    // rasterize_layer blit-stage order (aura strictly
                    // behind content); marks the handled flag so the CPU
                    // blit stage skips its own aura pass afterwards.
                    if let EvaluatedEffectType::DropShadow {
                        distance,
                        angle,
                        softness,
                        opacity,
                        color,
                    } = eff_type
                    {
                        let strength = (opacity / 100.0).clamp(0.0, 1.0) * 0.75;
                        if strength > 0.005 {
                            let scratch = self.scratch.as_ref().expect("targets allocated");
                            if let Some(fill) = self.shadow_fill.as_ref() {
                                // CPU blit ties the offset to the raster
                                // scale (kx, ky); the chain carries raw px
                                // (identical at 1:1 viewport scale).
                                let rad = angle.to_radians();
                                let ox = distance * rad.cos();
                                let oy = distance * rad.sin();
                                let uniforms = FxUniforms::pack(
                                    &[ox, oy, strength],
                                    &[[color.r, color.g, color.b, color.a]],
                                    time_s,
                                    width as f32,
                                    height as f32,
                                    0.0,
                                );
                                fill.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target().view(),
                                    scratch,
                                    uniforms,
                                );
                            }
                            let soft = softness.clamp(0.0, 8.0);
                            if soft > 0.5 {
                                self.blur_pipeline.record_blur(
                                    &self.gpu,
                                    &mut encoder,
                                    scratch,
                                    intermediate,
                                    targets.write_target(),
                                    soft,
                                );
                            } else if let Some(blit) = self.blit_pass.as_ref() {
                                blit.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    scratch.view(),
                                    targets.write_target(),
                                    FxUniforms::pack(&[], &[], time_s, width as f32, height as f32, 0.0),
                                );
                            }
                            if let Some(merge) = self.merge_pass_cache.get("under_prem") {
                                merge.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target().view(),
                                    targets.write_target().view(),
                                    intermediate,
                                    FxUniforms::pack(
                                        &[],
                                        &[[color.r, color.g, color.b, color.a]],
                                        time_s,
                                        width as f32,
                                        height as f32,
                                        0.0,
                                    ),
                                );
                            }
                            if let Some(blit) = self.blit_pass.as_ref() {
                                blit.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    intermediate.view(),
                                    targets.read_target(),
                                    FxUniforms::pack(&[], &[], time_s, width as f32, height as f32, 0.0),
                                );
                            }
                            set_aura_gpu_handled();
                        }
                        continue;
                    }
                    if let EvaluatedEffectType::OuterGlow {
                        size,
                        spread,
                        opacity,
                        color,
                        ..
                    } = eff_type
                    {
                        let strength = (opacity / 100.0).clamp(0.0, 1.0);
                        let radius = (*size).clamp(0.5, 8.0);
                        if strength > 0.01 && radius > 0.5 {
                            let scratch = self.scratch.as_ref().expect("targets allocated");
                            if let Some(fill) = self.glow_fill.as_ref() {
                                let uniforms = FxUniforms::pack(
                                    &[*spread, strength],
                                    &[[color.r, color.g, color.b, color.a]],
                                    time_s,
                                    width as f32,
                                    height as f32,
                                    0.0,
                                );
                                fill.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target().view(),
                                    scratch,
                                    uniforms,
                                );
                                self.blur_pipeline.record_blur(
                                    &self.gpu,
                                    &mut encoder,
                                    scratch,
                                    intermediate,
                                    targets.write_target(),
                                    radius,
                                );
                            }
                            if let Some(merge) = self.merge_pass_cache.get("under_prem") {
                                merge.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    targets.read_target().view(),
                                    targets.write_target().view(),
                                    intermediate,
                                    FxUniforms::pack(
                                        &[],
                                        &[[color.r, color.g, color.b, color.a]],
                                        time_s,
                                        width as f32,
                                        height as f32,
                                        0.0,
                                    ),
                                );
                            }
                            if let Some(blit) = self.blit_pass.as_ref() {
                                blit.record_into(
                                    &self.gpu,
                                    &mut encoder,
                                    intermediate.view(),
                                    targets.read_target(),
                                    FxUniforms::pack(&[], &[], time_s, width as f32, height as f32, 0.0),
                                );
                            }
                            set_aura_gpu_handled();
                        }
                        continue;
                    }
                    // Unsharp mask: compute-blur the chain cursor into
                    // scratch, then merge (orig, blurred) into write.
                    if let EvaluatedEffectType::Sharpen { amount, radius } = eff_type {
                        let k = (*amount / 100.0).clamp(0.0, 2.0);
                        if k > 0.01 {
                            if let (Some(merge), Some(scratch)) = (
                                self.merge_pass_cache.get("unsharp"),
                                self.scratch.as_ref(),
                            ) {
                                let uniforms = FxUniforms::pack(&[k], &[], time_s, width as f32, height as f32, 0.0);
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
                        EvaluatedEffectType::CelShading { levels, edge, highlight_intensity, shadow_intensity, highlight_tint_amount, shadow_tint_amount, .. } => {
                            // Scalars ride params[0..1]; tints ride the
                            // colors array (the 10-slot convention is full).
                            (
                                "cel_shading",
                                [*levels, *edge, *highlight_intensity, *shadow_intensity, *highlight_tint_amount, *shadow_tint_amount, 0.0, 0.0, 0.0, 0.0],
                            )
                        }
                        EvaluatedEffectType::OilPaint { radius, amount } => {
                            ("oil_paint", [*radius, *amount, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::MatchColor { amount, src_mid, src_high, .. } => {
                            // Zone RGBs ride colors[0..3] + params (the 10
                            // scalar slots hold amount + src mid/high; see
                            // the colors match below).
                            ("match_color", [*amount, src_mid.r, src_mid.g, src_mid.b, src_high.r, src_high.g, src_high.b, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::MatchLight { amount, src_mid, src_high, .. } => {
                            ("match_light", [*amount, src_mid.r, src_mid.g, src_mid.b, src_high.r, src_high.g, src_high.b, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::InnerGlow { size, intensity, blend, .. } => {
                            ("inner_glow", [*size, *intensity, blend.id(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::Dither { amount, levels } => {
                            ("dither", [*amount, *levels, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        EvaluatedEffectType::RimLight { mode, direction, width, intensity, blend, .. } => {
                            let m = match mode {
                                project::RimMode::Directional => 0.0,
                                project::RimMode::Full => 1.0,
                            };
                            ("rim_light", [m, *direction, *width, blend.id(), *intensity, 0.0, 0.0, 0.0, 0.0, 0.0])
                        }
                        // NOTE: no OuterGlow branch on purpose (blit-stage
                        // effect; a chain pass would double-apply, see the
                        // for_builtin note). It falls to `_ => continue`
                        // below, declining the chain to the exact CPU path.
                        EvaluatedEffectType::Checkerboard { size, color_a, color_b } => {
                            let s = size.max(2.0);
                            let cx = (width as f32 / s).max(1.0);
                            let cy = (height as f32 / s).max(1.0);
                            ("checkerboard", [cx, cy, 0.0, 0.0, color_a.r, color_a.g, color_a.b, color_b.r, color_b.g, color_b.b])
                        }
                        EvaluatedEffectType::GradientRamp { gradient, fill_alpha, .. } => {
                            let ca = gradient.stops.first().map(|s| s.color).unwrap_or(project::Color::BLACK);
                            let cb = gradient.stops.last().map(|s| s.color).unwrap_or(project::Color::WHITE);
                            // params[0]=ca+pad, params[1]=cb+pad, params[2]=(angle, fill).
                            // Angle sits at params[2].x to match the twin call
                            // below (it used to land at params[1].w, which the
                            // twin never read â€” GPU ramps were always 0Â°).
                            ("gradient_ramp", [ca.r, ca.g, ca.b, 0.0, cb.r, cb.g, cb.b, 0.0, gradient.angle.to_radians(), *fill_alpha as i32 as f32])
                        }
                        _ => continue,
                    };
                    // Cel tints ride the colors array (params convention is
                    // full at 10 slots); match zones take all four color
                    // slots (ref S/M/H + src shadow; src mid/high ride
                    // params); every other builtin uses none.
                    let tint_colors: [[f32; 4]; 4] = match eff_type {
                        EvaluatedEffectType::CelShading { highlight_tint, shadow_tint, .. } => [
                            [highlight_tint.r, highlight_tint.g, highlight_tint.b, 1.0],
                            [shadow_tint.r, shadow_tint.g, shadow_tint.b, 1.0],
                            [0.0; 4],
                            [0.0; 4],
                        ],
                        EvaluatedEffectType::MatchColor { ref_shadow, ref_mid, ref_high, src_shadow, .. }
                        | EvaluatedEffectType::MatchLight { ref_shadow, ref_mid, ref_high, src_shadow, .. } => [
                            [ref_shadow.r, ref_shadow.g, ref_shadow.b, 1.0],
                            [ref_mid.r, ref_mid.g, ref_mid.b, 1.0],
                            [ref_high.r, ref_high.g, ref_high.b, 1.0],
                            [src_shadow.r, src_shadow.g, src_shadow.b, 1.0],
                        ],
                        EvaluatedEffectType::InnerGlow { color, .. } => [
                            [color.r, color.g, color.b, 1.0],
                            [0.0; 4],
                            [0.0; 4],
                            [0.0; 4],
                        ],
                        EvaluatedEffectType::RimLight { color, .. } => [
                            [color.r, color.g, color.b, 1.0],
                            [0.0; 4],
                            [0.0; 4],
                            [0.0; 4],
                        ],
                        _ => [[0.0; 4]; 4],
                    };
                    if let Some(pass) = self.builtin_pass_cache.get(id) {
                        let uniforms = FxUniforms::pack(&params, &tint_colors, time_s, width as f32, height as f32, 0.0);
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
        let uniforms = FxUniforms::pack(&[k], &[], 0.0, width as f32, height as f32, 0.0);
        // 2. Bright-pass into write.
        bright.record_into(
            &self.gpu,
            &mut encoder,
            targets.read_target().view(),
            targets.write_target(),
            FxUniforms::pack(&[], &[], 0.0, width as f32, height as f32, 0.0),
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
            FxUniforms::pack(&[], &[], 0.0, width as f32, height as f32, 0.0),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// LightRays end to end on an opaque dark field with a bright bar:
    /// additive rays must appear tinted (covers the uniform path).
    #[test]
    fn rays_engine_additive() {
        let Ok(gpu) = GpuContext::new_headless() else { return };
        let Ok(mut engine) = GpuEffectEngine::new(gpu) else { return };
        // Opaque dark field with a bright horizontal bar (rows 2-3).
        let mut src = vec![0u8; 8 * 8 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let i = (y * 8 + x) * 4;
                src[i] = 10;
                src[i + 1] = 10;
                src[i + 2] = 10;
                src[i + 3] = 255;
            }
        }
        for y in 2..4 {
            for x in 0..8 {
                let i = (y * 8 + x) * 4;
                src[i] = 255;
                src[i + 1] = 255;
                src[i + 2] = 255;
            }
        }
        let eff = EvaluatedEffect {
            id: "rays".to_string(),
            name: "Rays".to_string(),
            enabled: true,
            effect_type: EvaluatedEffectType::Stock {
                plugin: StockPlugin::LightRays,
                params: vec![50.0, 20.0, 90.0, 20.0, 12.0, 60.0, 96.0, 80.0, 24.0, 0.0, 0.0, 80.0],
                colors: vec![project::Color::rgba(1.0, 0.9, 0.7, 1.0)],
            },
        };
        assert!(engine.supports_fx_chain(std::slice::from_ref(&eff)));
        let out = engine.process_rgba_frame(&src, 8, 8, std::slice::from_ref(&eff), 0.0).expect("process");
        let j = (6 * 8 + 4) * 4;
        eprintln!("RAYS-ENGINE dark px={:?}", &out[j..j + 4]);
        assert!(out[j] > 20, "rays must reach dark areas, got {:?}", &out[j..j + 4]);
    }

    /// GPU compute blur vs the CPU fallback kernel on identical bytes:
    /// outputs must agree (same math both sides, guards the shared-blur
    /// parity the multi-stage stock paths rely on).
    #[test]
    fn blur_paths_agree() {
        let Ok(gpu) = GpuContext::new_headless() else { return };
        let Ok(mut engine) = GpuEffectEngine::new(gpu) else { return };
        // White mask (64 gray) on transparent, 16x16 with a bright block.
        let mut src = vec![0u8; 16 * 16 * 4];
        for y in 4..12 {
            for x in 4..12 {
                let i = (y * 16 + x) * 4;
                src[i] = 64;
                src[i + 1] = 64;
                src[i + 2] = 64;
                src[i + 3] = 64;
            }
        }
        let gpu_out = engine.blur_rgba(&src, 16, 16, 4.0).expect("blur");
        let mut cpu_out = src.clone();
        crate::blur::gaussian_blur_rgba(&mut cpu_out, 16, 16, 4.0);
        let mut worst = 0i32;
        let mut at = 0usize;
        for i in 0..cpu_out.len() {
            let d = (cpu_out[i] as i32 - gpu_out[i] as i32).abs();
            if d > worst {
                worst = d;
                at = i;
            }
        }
        eprintln!("BLUR-PATHS worst={worst} at byte {at} cpu={} gpu={}", cpu_out[at], gpu_out[at]);
        assert!(worst <= 2, "blur paths must agree, worst {worst}");
    }
}
