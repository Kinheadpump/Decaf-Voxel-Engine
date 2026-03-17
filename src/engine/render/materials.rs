use std::collections::HashMap;
use std::path::Path;

use crate::engine::core::types::MAX_TEXTURE_LAYERS;
use crate::engine::world::block::registry::BlockRegistry;
use anyhow::{Context, bail};

#[derive(Clone, Default)]
pub struct TextureRegistry {
    layers: HashMap<String, u16>,
    names_by_layer: Vec<String>,
}

impl TextureRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>) -> u16 {
        let name = name.into();
        if let Some(&layer) = self.layers.get(&name) {
            return layer;
        }

        let layer = self.names_by_layer.len() as u16;
        assert!(
            u32::from(layer) < MAX_TEXTURE_LAYERS,
            "texture registry exceeds the packed face layer limit of {MAX_TEXTURE_LAYERS}",
        );

        self.names_by_layer.push(name.clone());
        self.layers.insert(name, layer);
        layer
    }

    pub fn layer_map(&self) -> &HashMap<String, u16> {
        &self.layers
    }

    pub fn name_for_layer(&self, layer: u16) -> Option<&str> {
        self.names_by_layer.get(layer as usize).map(|name| name.as_str())
    }

    pub fn layer_count(&self) -> u16 {
        self.names_by_layer.len() as u16
    }
}

pub fn create_texture_registry(blocks: &BlockRegistry) -> TextureRegistry {
    let mut reg = TextureRegistry::new();

    for block in blocks.iter() {
        block.textures.visit_refs(|texture| {
            reg.register(texture.0.clone());
        });
    }

    reg
}

pub struct Materials {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Materials {
    pub fn from_texture_registry(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures: &TextureRegistry,
    ) -> anyhow::Result<Self> {
        let texture_dir = Path::new("assets").join("textures");
        let layer_count = textures.layer_count().max(1);
        let mut rgba = Vec::new();
        let mut texture_size = None;

        for layer in 0..layer_count {
            let name = textures.name_for_layer(layer).unwrap_or("air");
            let path = texture_dir.join(format!("{name}.png"));
            let image = image::open(&path)
                .with_context(|| {
                    format!("failed to load texture '{name}' from {}", path.display())
                })?
                .into_rgba8();
            let (width, height) = image.dimensions();

            match texture_size {
                Some((expected_width, expected_height))
                    if expected_width != width || expected_height != height =>
                {
                    bail!(
                        "texture '{}' has size {}x{}, expected {}x{} for all texture layers",
                        name,
                        width,
                        height,
                        expected_width,
                        expected_height
                    );
                }
                None => {
                    texture_size = Some((width, height));
                }
                _ => {}
            }

            rgba.extend_from_slice(image.as_raw());
        }

        let (width, height) = texture_size.unwrap_or((1, 1));

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("materials_texture_array"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: layer_count as u32 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: layer_count as u32 },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("materials_texture_array_view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("materials_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Ok(Self { texture, view, sampler })
    }

    pub fn keep_alive(&self) -> &wgpu::Texture {
        &self.texture
    }
}
