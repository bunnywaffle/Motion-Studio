pub mod device;

pub use device::{GpuContext, GpuError, RenderTarget};

#[cfg(test)]
mod tests {
    use super::*;

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
}
