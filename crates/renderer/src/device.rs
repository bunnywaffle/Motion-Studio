use thiserror::Error;

#[derive(Error, Debug)]
pub enum GpuError {
    #[error("Failed to request adapter: no suitable GPU adapter found")]
    NoAdapterFound,
    #[error("Failed to request GPU device: {0}")]
    RequestDeviceError(#[from] wgpu::RequestDeviceError),
    #[error("Buffer mapping failed: {0}")]
    BufferAsyncError(String),
    #[error("Texture dimensions invalid: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
}

/// GPU Context encapsulating wgpu instance, adapter, device, and command queue.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Initialize a headless GPU context with fallback to software/GL if discrete GPU is unavailable.
    pub fn new_headless() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // First attempt: HighPerformance / default adapter
        let adapter_opt = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }));

        // Fallback attempt: Software / fallback adapter if discrete is not available
        let adapter = match adapter_opt {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                compatible_surface: None,
                force_fallback_adapter: true,
            }))
            .ok_or(GpuError::NoAdapterFound)?,
        };

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Motion Compositor Headless Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Return info about the selected adapter.
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }

    /// Reference to the wgpu device.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Reference to the wgpu command queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// An offscreen rendering destination texture and view with CPU readback capabilities.
pub struct RenderTarget {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTarget {
    pub const DEFAULT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// Allocate a new offscreen render target texture at the given width and height.
    pub fn new(gpu: &GpuContext, width: u32, height: u32) -> Result<Self, GpuError> {
        if width == 0 || height == 0 {
            return Err(GpuError::InvalidDimensions { width, height });
        }

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let format = Self::DEFAULT_FORMAT;

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("RenderTarget Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            width,
            height,
            format,
            texture,
            view,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Read the render target texture back to CPU memory (RGBA8 format).
    /// Respects the 256-byte alignment requirement for wgpu texture copy.
    pub fn read_texture_to_cpu(&self, gpu: &GpuContext) -> Result<Vec<u8>, GpuError> {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT; // 256
        let unpadded_bytes_per_row = self.width * 4;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let buffer_size = (padded_bytes_per_row * self.height) as u64;

        let staging_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Readback Staging Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Readback Encoder"),
            });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        gpu.queue.submit(std::iter::once(encoder.finish()));

        let slice = staging_buffer.slice(..);
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

        // Extract tightly packed RGBA bytes
        let mut tightly_packed = Vec::with_capacity((self.width * self.height * 4) as usize);
        for row in 0..self.height {
            let start = (row * padded_bytes_per_row) as usize;
            let end = start + unpadded_bytes_per_row as usize;
            tightly_packed.extend_from_slice(&mapped_view[start..end]);
        }

        drop(mapped_view);
        staging_buffer.unmap();

        Ok(tightly_packed)
    }
}
