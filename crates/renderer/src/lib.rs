pub mod blit;
pub mod blur;
pub mod cache;
pub mod device;
pub mod shader;

pub use blit::{BlitPipeline, BlitUniforms};
pub use blur::{gaussian_blur_rgba, gaussian_kernel_1d, BLUR_WGSL};
pub use cache::{CachedTexture, TextureCache};
pub use device::{DoubleBufferedTarget, GpuContext, GpuError, RenderTarget};
pub use shader::{CustomShaderPipeline, CustomShaderUniforms};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_shader_pipeline() {
        if let Ok(gpu) = GpuContext::new_headless() {
            let target = RenderTarget::new(&gpu, 64, 64).expect("create target");
            let pipeline = CustomShaderPipeline::new(
                &gpu,
                CustomShaderPipeline::DEFAULT_WGSL,
                target.format(),
            )
            .expect("compile shader pipeline");

            let uniforms = CustomShaderUniforms::default();
            pipeline.render(&gpu, &target, uniforms);

            let pixels = target.read_texture_to_cpu(&gpu).expect("read pixels");
            assert_eq!(pixels.len(), 64 * 64 * 4);
        }
    }

    #[test]
    fn test_gpu_context_headless_initialization() {
        match GpuContext::new_headless() {
            Ok(gpu) => {
                let info = gpu.adapter_info();
                println!("Headless GPU initialized successfully: {} ({:?})", info.name, info.backend);
                assert!(!info.name.is_empty() || info.backend != wgpu::Backend::Empty);

                // Test RenderTarget allocation
                let target = RenderTarget::new(&gpu, 128, 64).expect("allocate render target");
                assert_eq!(target.width(), 128);
                assert_eq!(target.height(), 64);
                assert_eq!(target.format(), RenderTarget::DEFAULT_FORMAT);

                // Test CPU readback
                let pixels = target.read_texture_to_cpu(&gpu).expect("read texture to cpu");
                assert_eq!(pixels.len(), 128 * 64 * 4);
            }
            Err(GpuError::NoAdapterFound) => {
                println!("No GPU adapter available in this CI/environment; test skipped gracefully");
            }
            Err(e) => {
                panic!("Unexpected error initializing headless GPU context: {e}");
            }
        }
    }

    #[test]
    fn test_render_target_invalid_dimensions() {
        if let Ok(gpu) = GpuContext::new_headless() {
            let res = RenderTarget::new(&gpu, 0, 100);
            assert!(matches!(res, Err(GpuError::InvalidDimensions { width: 0, height: 100 })));

            let res2 = RenderTarget::new(&gpu, 100, 0);
            assert!(matches!(res2, Err(GpuError::InvalidDimensions { width: 100, height: 0 })));
        }
    }

    #[test]
    fn test_render_target_formats_and_clearing() {
        if let Ok(gpu) = GpuContext::new_headless() {
            let target = RenderTarget::with_format(&gpu, 16, 16, wgpu::TextureFormat::Rgba8Unorm)
                .expect("create target with format");
            assert_eq!(target.width(), 16);
            assert_eq!(target.height(), 16);
            assert_eq!(target.format(), wgpu::TextureFormat::Rgba8Unorm);

            // Clear to solid red
            target.clear(&gpu, wgpu::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });

            let pixels = target.read_texture_to_cpu(&gpu).expect("read pixels");
            assert_eq!(pixels.len(), 16 * 16 * 4);
            // Check top-left pixel is red
            assert_eq!(pixels[0], 255); // R
            assert_eq!(pixels[1], 0);   // G
            assert_eq!(pixels[2], 0);   // B
            assert_eq!(pixels[3], 255); // A
        }
    }

    #[test]
    fn test_double_buffered_target_ping_pong() {
        if let Ok(gpu) = GpuContext::new_headless() {
            let mut double_target = DoubleBufferedTarget::new(&gpu, 32, 32).expect("create double buffer");
            assert_eq!(double_target.width(), 32);
            assert_eq!(double_target.height(), 32);

            let first_read_view = double_target.read_target().view() as *const _;
            let first_write_view = double_target.write_target().view() as *const _;
            assert_ne!(first_read_view, first_write_view);

            // Swap buffers
            double_target.swap();
            let second_read_view = double_target.read_target().view() as *const _;
            let second_write_view = double_target.write_target().view() as *const _;

            assert_eq!(first_read_view, second_write_view);
            assert_eq!(first_write_view, second_read_view);
        }
    }

    #[test]
    fn test_texture_cache_upload_and_reuse() {
        if let Ok(gpu) = GpuContext::new_headless() {
            let mut cache = TextureCache::new();
            assert_eq!(cache.len(), 0);
            assert!(cache.is_empty());

            // 4x4 green pixels
            let green_pixels = [0u8, 255, 0, 255].repeat(16);
            let _view = cache
                .upload_rgba_image(&gpu, "test_green", 4, 4, &green_pixels)
                .expect("upload image");

            assert_eq!(cache.len(), 1);
            assert_eq!(cache.get_dimensions("test_green"), Some((4, 4)));
            assert!(cache.get_view("test_green").is_some());

            // Remove
            let removed = cache.remove("test_green");
            assert!(removed.is_some());
            assert_eq!(cache.len(), 0);
        }
    }

    #[test]
    fn test_blit_pipeline_rendering_and_readback() {
        if let Ok(gpu) = GpuContext::new_headless() {
            let mut cache = TextureCache::new();
            // 8x8 blue pixels
            let blue_pixels = [0u8, 0, 255, 255].repeat(64);
            cache
                .upload_rgba_image(&gpu, "test_blue", 8, 8, &blue_pixels)
                .expect("upload image");
            let source_view = cache.get_view("test_blue").unwrap();

            let target = RenderTarget::new(&gpu, 8, 8).expect("create render target");
            let blit = BlitPipeline::new(&gpu, target.format());

            // Blit with identity uniforms and clear to black first
            blit.render(
                &gpu,
                source_view,
                &target,
                BlitUniforms::default(),
                Some(wgpu::Color::BLACK),
            );

            let pixels = target.read_texture_to_cpu(&gpu).expect("read blitted pixels");
            assert_eq!(pixels.len(), 8 * 8 * 4);
            // Verify pixel is predominantly blue
            assert_eq!(pixels[0], 0);
            assert_eq!(pixels[1], 0);
            assert!(pixels[2] > 200); // Blue
            assert_eq!(pixels[3], 255);
        }
    }
}

