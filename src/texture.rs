use std::num::NonZeroU32;

use crate::types::{DEPTH_FORMAT, VIEW_COUNT};

pub struct Texture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Texture {
    pub fn from_wgpu(texture: wgpu::Texture, view: wgpu::TextureView) -> Self {
        Self {
            _texture: texture,
            view,
        }
    }

    pub fn new_rt_texture(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        texture_format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Render Target Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: NonZeroU32::new(VIEW_COUNT),
            ..Default::default()
        });
        Self {
            _texture: texture,
            view,
        }
    }
    pub fn new_depth_texture(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: NonZeroU32::new(VIEW_COUNT),
            ..Default::default()
        });
        Self {
            _texture: texture,
            view,
        }
    }
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}
