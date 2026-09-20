use crate::device::{GpuContext, GpuError};
use std::collections::HashMap;

pub struct CachedTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
}

/// Texture cache for caching uploaded assets (images/video frames) and pre-rendered compositions.
pub struct TextureCache {
    textures: HashMap<String, CachedTexture>,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    /// Number of cached textures.
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    /// Look up a cached texture view by key.
    pub fn get_view(&self, key: &str) -> Option<&wgpu::TextureView> {
        self.textures.get(key).map(|t| &t.view)
    }

    /// Look up cached texture dimensions (width, height) by key.
    pub fn get_dimensions(&self, key: &str) -> Option<(u32, u32)> {
        self.textures.get(key).map(|t| (t.width, t.height))
    }

    /// Look up a full cached texture entry by key.
    pub fn get(&self, key: &str) -> Option<&CachedTexture> {
        self.textures.get(key)
    }

    /// Remove a cached texture by key.
    pub fn remove(&mut self, key: &str) -> Option<CachedTexture> {
        self.textures.remove(key)
    }

    /// Clear all cached textures.
    pub fn clear(&mut self) {
        self.textures.clear();
    }

    /// Upload an RGBA8 buffer to the GPU and cache it under `key`.
    pub fn upload_rgba_image(
        &mut self,
        gpu: &GpuContext,
        key: &str,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> Result<&wgpu::TextureView, GpuError> {
        if width == 0 || height == 0 {
            return Err(GpuError::InvalidDimensions { width, height });
        }

        let expected_size = (width * height * 4) as usize;
        if data.len() < expected_size {
            return Err(GpuError::BufferAsyncError(format!(
                "Data slice too short: expected {} bytes, got {}",
                expected_size,
                data.len()
            )));
        }

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("CachedTexture_{key}")),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.textures.insert(
            key.to_string(),
            CachedTexture {
                texture,
                view,
                width,
                height,
                format,
            },
        );

        Ok(&self.textures.get(key).unwrap().view)
    }
}
