pub struct Materials {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Materials {
    pub fn create_dummy(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layer_count = 128u32;
        let size = 16u32;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block_texture_array"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layer_count,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let mut data = vec![0u8; (size * size * layer_count * 4) as usize];

        for layer in 0..layer_count {
            let color = match layer % 8 {
                0 => [255, 255, 255, 255],
                1 => [120, 80, 50, 255],
                2 => [60, 180, 60, 255],
                3 => [180, 180, 180, 255],
                4 => [200, 150, 60, 255],
                5 => [80, 120, 220, 255],
                6 => [140, 70, 160, 255],
                _ => [220, 90, 90, 255],
            };

            let base = (layer * size * size * 4) as usize;
            for i in 0..(size * size) as usize {
                let px = base + i * 4;
                data[px + 0] = color[0];
                data[px + 1] = color[1];
                data[px + 2] = color[2];
                data[px + 3] = color[3];
            }
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layer_count,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("block_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            ..Default::default()
        });

        Self { texture, view, sampler }
    }
}