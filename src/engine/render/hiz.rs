use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use crossbeam_channel::{Receiver, Sender, unbounded};
use glam::{UVec2, Vec2, Vec4};

use crate::{
    config::HiZOcclusionConfig,
    engine::{
        core::math::{Mat4, Vec3},
        render::{camera::Camera, targets::DepthTarget},
    },
};

pub struct HiZOcclusion {
    settings: HiZOcclusionConfig,
    texture: wgpu::Texture,
    views: Vec<wgpu::TextureView>,
    mip_sizes: Vec<UVec2>,
    copy_layouts: Vec<MipCopyLayout>,
    readback_slots: Vec<ReadbackSlot>,
    ready_tx: Sender<(usize, bool)>,
    ready_rx: Receiver<(usize, bool)>,
    next_readback_slot: usize,

    pass_uniform_buffer: wgpu::Buffer,
    pass_uniform_stride: u64,
    empty_bind_group: wgpu::BindGroup,
    depth_reduce_bind_group_layout: wgpu::BindGroupLayout,
    downsample_bind_group_layout: wgpu::BindGroupLayout,
    depth_reduce_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,

    cpu_pyramid: Option<CpuHiZPyramid>,
    previous_camera: Option<Camera>,
}

impl HiZOcclusion {
    pub fn new(
        device: &wgpu::Device,
        surface_width: u32,
        surface_height: u32,
        settings: HiZOcclusionConfig,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hiz_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hiz.wgsl").into()),
        });

        let uniform_size = std::mem::size_of::<HiZPassUniform>() as u64;
        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let pass_uniform_stride = uniform_size.div_ceil(uniform_alignment) * uniform_alignment;
        let pass_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hiz_pass_uniform_buffer"),
            size: pass_uniform_stride * settings.max_passes as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_reduce_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hiz_depth_reduce_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(non_zero_u64(uniform_size, "hiz uniform size")),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Depth,
                        },
                        count: None,
                    },
                ],
            });
        let downsample_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hiz_downsample_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(non_zero_u64(uniform_size, "hiz uniform size")),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        },
                        count: None,
                    },
                ],
            });
        let empty_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hiz_empty_bgl"),
                entries: &[],
            });
        let empty_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hiz_empty_bg"),
            layout: &empty_bind_group_layout,
            entries: &[],
        });

        let depth_reduce_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("hiz_depth_reduce_pipeline_layout"),
                bind_group_layouts: &[&depth_reduce_bind_group_layout],
                push_constant_ranges: &[],
            });
        let downsample_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("hiz_downsample_pipeline_layout"),
                bind_group_layouts: &[&empty_bind_group_layout, &downsample_bind_group_layout],
                push_constant_ranges: &[],
            });

        let depth_reduce_pipeline = create_hiz_pipeline(
            device,
            &depth_reduce_pipeline_layout,
            &shader,
            "fs_reduce_depth",
            "hiz_depth_reduce_pipeline",
        );
        let downsample_pipeline = create_hiz_pipeline(
            device,
            &downsample_pipeline_layout,
            &shader,
            "fs_reduce_hiz",
            "hiz_downsample_pipeline",
        );

        let (ready_tx, ready_rx) = unbounded();
        let mut this = Self {
            settings,
            texture: create_hiz_texture(device, UVec2::ONE, 1),
            views: Vec::new(),
            mip_sizes: Vec::new(),
            copy_layouts: Vec::new(),
            readback_slots: Vec::new(),
            ready_tx,
            ready_rx,
            next_readback_slot: 0,
            pass_uniform_buffer,
            pass_uniform_stride,
            empty_bind_group,
            depth_reduce_bind_group_layout,
            downsample_bind_group_layout,
            depth_reduce_pipeline,
            downsample_pipeline,
            cpu_pyramid: None,
            previous_camera: None,
        };
        this.resize(device, surface_width, surface_height);
        crate::log_info!(
            "Hi-Z occlusion enabled at {}x{} ({} mips)",
            this.mip_sizes[0].x,
            this.mip_sizes[0].y,
            this.mip_sizes.len()
        );
        this
    }

    pub fn resize(&mut self, device: &wgpu::Device, surface_width: u32, surface_height: u32) {
        let base_size =
            base_hiz_size(surface_width, surface_height, self.settings.max_base_dimension);
        let mip_sizes = build_mip_sizes(base_size);
        let mip_count = mip_sizes.len() as u32;

        self.texture = create_hiz_texture(device, base_size, mip_count);
        self.views = (0..mip_count)
            .map(|mip_level| {
                self.texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("hiz_mip_view"),
                    format: Some(wgpu::TextureFormat::R32Float),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: mip_level,
                    mip_level_count: Some(1),
                    base_array_layer: 0,
                    array_layer_count: Some(1),
                })
            })
            .collect();
        self.copy_layouts = build_copy_layouts(&mip_sizes);
        self.readback_slots = (0..self.settings.readback_slots)
            .map(|_| ReadbackSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("hiz_readback_buffer"),
                    size: total_copy_size(&self.copy_layouts),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                pending: false,
            })
            .collect();
        let (ready_tx, ready_rx) = unbounded();
        self.ready_tx = ready_tx;
        self.ready_rx = ready_rx;
        self.next_readback_slot = 0;
        self.mip_sizes = mip_sizes;
        self.cpu_pyramid = None;
        self.previous_camera = None;
    }

    pub fn update_readback(&mut self, device: &wgpu::Device) {
        let _span = crate::profile_span!("hiz::update_readback");

        let _ = device.poll(wgpu::Maintain::Poll);

        while let Ok((slot_index, is_ok)) = self.ready_rx.try_recv() {
            if slot_index >= self.readback_slots.len() {
                continue;
            }

            if is_ok {
                if let Err(err) = self.consume_readback(slot_index) {
                    crate::log_warn!("failed to consume Hi-Z readback: {err:#}");
                }
            } else if let Some(slot) = self.readback_slots.get_mut(slot_index) {
                slot.pending = false;
                slot.buffer.unmap();
                crate::log_warn!("Hi-Z readback mapping failed");
            }
        }
    }

    pub fn is_chunk_occluded(&self, camera: &Camera, min: Vec3, max: Vec3) -> bool {
        let Some(previous_camera) = self.previous_camera else {
            return false;
        };
        let Some(cpu_pyramid) = &self.cpu_pyramid else {
            return false;
        };

        if !can_reuse_previous_camera(&self.settings, camera, &previous_camera) {
            return false;
        }

        if distance_sq_to_aabb(camera.position, min, max)
            < self.settings.near_chunk_skip_distance * self.settings.near_chunk_skip_distance
        {
            return false;
        }

        let Some(projected) = project_aabb(previous_camera.view_proj(), min, max) else {
            return false;
        };
        let level = cpu_pyramid.choose_level(projected.uv_max - projected.uv_min);
        let mip_size = cpu_pyramid.mip_size(level);

        let x0 = ((projected.uv_min.x * mip_size.x as f32).floor() as i32 - 1)
            .clamp(0, mip_size.x as i32 - 1) as u32;
        let y0 = ((projected.uv_min.y * mip_size.y as f32).floor() as i32 - 1)
            .clamp(0, mip_size.y as i32 - 1) as u32;
        let x1 = ((projected.uv_max.x * mip_size.x as f32).ceil() as i32 + 1)
            .clamp(1, mip_size.x as i32) as u32;
        let y1 = ((projected.uv_max.y * mip_size.y as f32).ceil() as i32 + 1)
            .clamp(1, mip_size.y as i32) as u32;

        if x0 >= x1 || y0 >= y1 {
            return false;
        }

        let min_depth = cpu_pyramid.min_depth(level, x0, y0, x1, y1);
        min_depth > projected.nearest_depth + self.settings.occlusion_depth_bias
    }

    pub fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth_target: &DepthTarget,
        surface_width: u32,
        surface_height: u32,
    ) -> Option<usize> {
        let _span = crate::profile_span!("hiz::record");

        if self.views.is_empty() {
            return None;
        }

        let pass_uniforms =
            build_pass_uniforms(UVec2::new(surface_width, surface_height), &self.mip_sizes);
        if pass_uniforms.len() > self.settings.max_passes {
            crate::log_warn!(
                "Skipping Hi-Z update because {} passes exceed the configured {}-pass limit",
                pass_uniforms.len(),
                self.settings.max_passes
            );
            return None;
        }

        let mut uniform_bytes = vec![0u8; pass_uniforms.len() * self.pass_uniform_stride as usize];
        let uniform_size = std::mem::size_of::<HiZPassUniform>();
        for (index, pass_uniform) in pass_uniforms.iter().enumerate() {
            let offset = index * self.pass_uniform_stride as usize;
            uniform_bytes[offset..offset + uniform_size]
                .copy_from_slice(bytemuck::bytes_of(pass_uniform));
        }
        queue.write_buffer(&self.pass_uniform_buffer, 0, &uniform_bytes);

        let depth_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hiz_depth_reduce_bg"),
            layout: &self.depth_reduce_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.pass_uniform_buffer,
                        offset: 0,
                        size: Some(non_zero_u64(
                            std::mem::size_of::<HiZPassUniform>() as u64,
                            "hiz pass uniform binding size",
                        )),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&depth_target.view),
                },
            ],
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hiz_depth_reduce_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.views[0],
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.depth_reduce_pipeline);
            pass.set_bind_group(0, &depth_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        for mip_level in 1..self.views.len() {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hiz_downsample_bg"),
                layout: &self.downsample_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.pass_uniform_buffer,
                            offset: mip_level as u64 * self.pass_uniform_stride,
                            size: Some(non_zero_u64(
                                std::mem::size_of::<HiZPassUniform>() as u64,
                                "hiz pass uniform binding size",
                            )),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.views[mip_level - 1]),
                    },
                ],
            });

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hiz_downsample_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.views[mip_level],
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.downsample_pipeline);
            pass.set_bind_group(0, &self.empty_bind_group, &[]);
            pass.set_bind_group(1, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        let slot_index = self.reserve_readback_slot()?;
        let slot = &self.readback_slots[slot_index];

        for (mip_level, layout) in self.copy_layouts.iter().enumerate() {
            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    texture: &self.texture,
                    mip_level: mip_level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &slot.buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: layout.offset,
                        bytes_per_row: Some(layout.bytes_per_row),
                        rows_per_image: Some(layout.height),
                    },
                },
                wgpu::Extent3d {
                    width: layout.width,
                    height: layout.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        Some(slot_index)
    }

    pub fn start_readback(&mut self, slot_index: usize) {
        let sender = self.ready_tx.clone();
        self.readback_slots[slot_index].buffer.slice(..).map_async(
            wgpu::MapMode::Read,
            move |result| {
                let _ = sender.send((slot_index, result.is_ok()));
            },
        );
    }

    pub fn finish_frame(&mut self, camera: &Camera) {
        self.previous_camera = Some(*camera);
    }

    fn reserve_readback_slot(&mut self) -> Option<usize> {
        for _ in 0..self.readback_slots.len() {
            let slot_index = self.next_readback_slot;
            self.next_readback_slot = (self.next_readback_slot + 1) % self.readback_slots.len();

            if !self.readback_slots[slot_index].pending {
                self.readback_slots[slot_index].pending = true;
                return Some(slot_index);
            }
        }

        crate::log_debug!(
            "Skipping Hi-Z readback for this frame because all staging slots are busy"
        );
        None
    }

    fn consume_readback(&mut self, slot_index: usize) -> anyhow::Result<()> {
        let slot = &mut self.readback_slots[slot_index];
        let mapped = slot.buffer.slice(..).get_mapped_range();
        let mut values = Vec::new();
        let mut offsets = Vec::with_capacity(self.copy_layouts.len());

        for layout in &self.copy_layouts {
            offsets.push(values.len());

            for row in 0..layout.height {
                let row_start =
                    layout.offset as usize + row as usize * layout.bytes_per_row as usize;
                let row_end = row_start + layout.width as usize * std::mem::size_of::<f32>();
                values.extend_from_slice(bytemuck::cast_slice(&mapped[row_start..row_end]));
            }
        }

        drop(mapped);
        slot.buffer.unmap();
        slot.pending = false;

        self.cpu_pyramid =
            Some(CpuHiZPyramid { mip_sizes: self.mip_sizes.clone(), offsets, values });
        Ok(())
    }
}

#[derive(Clone)]
struct CpuHiZPyramid {
    mip_sizes: Vec<UVec2>,
    offsets: Vec<usize>,
    values: Vec<f32>,
}

impl CpuHiZPyramid {
    fn choose_level(&self, uv_extent: Vec2) -> usize {
        let base_size = self.mip_sizes[0].as_vec2();
        let texel_extent = uv_extent.max(Vec2::splat(0.0)) * base_size;
        let max_extent = texel_extent.max_element().max(1.0);
        (max_extent.log2().floor() as usize).min(self.mip_sizes.len().saturating_sub(1))
    }

    fn mip_size(&self, level: usize) -> UVec2 {
        self.mip_sizes[level]
    }

    fn min_depth(&self, level: usize, x0: u32, y0: u32, x1: u32, y1: u32) -> f32 {
        let mip_size = self.mip_sizes[level];
        let row_width = mip_size.x as usize;
        let start = self.offsets[level];
        let end = start + row_width * mip_size.y as usize;
        let mip = &self.values[start..end];

        let mut min_depth: f32 = 1.0;

        for y in y0..y1 {
            let row = y as usize * row_width;
            for x in x0..x1 {
                min_depth = min_depth.min(mip[row + x as usize]);
            }
        }

        min_depth
    }
}

#[derive(Clone, Copy)]
struct ProjectedAabb {
    uv_min: Vec2,
    uv_max: Vec2,
    nearest_depth: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HiZPassUniform {
    src_size: [u32; 2],
    dst_size: [u32; 2],
}

#[derive(Clone, Copy)]
struct MipCopyLayout {
    width: u32,
    height: u32,
    offset: u64,
    bytes_per_row: u32,
}

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    pending: bool,
}

fn create_hiz_texture(device: &wgpu::Device, size: UVec2, mip_count: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hiz_texture"),
        size: wgpu::Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
        mip_level_count: mip_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn create_hiz_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry_point: &'static str,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: fragment_entry_point,
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::R32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::RED,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn base_hiz_size(surface_width: u32, surface_height: u32, max_base_dimension: u32) -> UVec2 {
    let mut width = surface_width.max(1);
    let mut height = surface_height.max(1);

    while width > max_base_dimension || height > max_base_dimension {
        width = width.div_ceil(2);
        height = height.div_ceil(2);
    }

    UVec2::new(width.max(1), height.max(1))
}

fn build_mip_sizes(base_size: UVec2) -> Vec<UVec2> {
    let mut mip_sizes = Vec::new();
    let mut current = base_size;

    loop {
        mip_sizes.push(current);
        if current == UVec2::ONE {
            break;
        }

        current = UVec2::new((current.x / 2).max(1), (current.y / 2).max(1));
    }

    mip_sizes
}

fn build_pass_uniforms(surface_size: UVec2, mip_sizes: &[UVec2]) -> Vec<HiZPassUniform> {
    let mut passes = Vec::with_capacity(mip_sizes.len());
    passes.push(HiZPassUniform {
        src_size: surface_size.to_array(),
        dst_size: mip_sizes[0].to_array(),
    });

    for mip_level in 1..mip_sizes.len() {
        passes.push(HiZPassUniform {
            src_size: mip_sizes[mip_level - 1].to_array(),
            dst_size: mip_sizes[mip_level].to_array(),
        });
    }

    passes
}

fn build_copy_layouts(mip_sizes: &[UVec2]) -> Vec<MipCopyLayout> {
    let mut offset = 0u64;
    let mut layouts = Vec::with_capacity(mip_sizes.len());

    for mip_size in mip_sizes {
        let bytes_per_row = align_to(
            mip_size.x as u64 * std::mem::size_of::<f32>() as u64,
            wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64,
        ) as u32;

        offset = align_to(offset, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
        layouts.push(MipCopyLayout {
            width: mip_size.x,
            height: mip_size.y,
            offset,
            bytes_per_row,
        });
        offset += bytes_per_row as u64 * mip_size.y as u64;
    }

    layouts
}

fn total_copy_size(layouts: &[MipCopyLayout]) -> u64 {
    layouts
        .last()
        .map(|layout| layout.offset + layout.bytes_per_row as u64 * layout.height as u64)
        .unwrap_or(0)
}

fn align_to(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }

    value.div_ceil(alignment) * alignment
}

fn non_zero_u64(value: u64, label: &str) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap_or_else(|| panic!("{label} must be non-zero"))
}

fn project_aabb(view_proj: Mat4, min: Vec3, max: Vec3) -> Option<ProjectedAabb> {
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ];

    let mut min_ndc = Vec2::splat(f32::INFINITY);
    let mut max_ndc = Vec2::splat(f32::NEG_INFINITY);
    let mut nearest_depth: f32 = 0.0;

    for corner in corners {
        let clip = view_proj * Vec4::new(corner.x, corner.y, corner.z, 1.0);
        if clip.w <= 0.0 {
            return None;
        }

        let ndc = clip.truncate() / clip.w;
        min_ndc = min_ndc.min(ndc.truncate());
        max_ndc = max_ndc.max(ndc.truncate());
        nearest_depth = nearest_depth.max(ndc.z);
    }

    if nearest_depth <= 0.0 {
        return None;
    }

    if max_ndc.x < -1.0 || min_ndc.x > 1.0 || max_ndc.y < -1.0 || min_ndc.y > 1.0 {
        return None;
    }

    let min_ndc = min_ndc.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
    let max_ndc = max_ndc.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));

    let uv_min = Vec2::new((min_ndc.x + 1.0) * 0.5, (1.0 - max_ndc.y) * 0.5);
    let uv_max = Vec2::new((max_ndc.x + 1.0) * 0.5, (1.0 - min_ndc.y) * 0.5);

    if uv_max.x <= uv_min.x || uv_max.y <= uv_min.y {
        return None;
    }

    Some(ProjectedAabb { uv_min, uv_max, nearest_depth: nearest_depth.clamp(0.0, 1.0) })
}

fn distance_sq_to_aabb(point: Vec3, min: Vec3, max: Vec3) -> f32 {
    let clamped = point.clamp(min, max);
    (point - clamped).length_squared()
}

fn can_reuse_previous_camera(
    settings: &HiZOcclusionConfig,
    camera: &Camera,
    previous_camera: &Camera,
) -> bool {
    if !camera.projection_matches(previous_camera) {
        return false;
    }

    if (camera.position - previous_camera.position).length_squared()
        > settings.max_camera_reuse_distance * settings.max_camera_reuse_distance
    {
        return false;
    }

    camera.forward.dot(previous_camera.forward) >= settings.min_camera_reuse_forward_dot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CameraConfig;

    #[test]
    fn surface_size_is_downscaled_for_hiz() {
        let settings = HiZOcclusionConfig::default();
        assert_eq!(base_hiz_size(1280, 720, settings.max_base_dimension), UVec2::new(160, 90));
        assert_eq!(base_hiz_size(256, 144, settings.max_base_dimension), UVec2::new(256, 144));
    }

    #[test]
    fn reverse_z_occlusion_uses_min_depth() {
        let pyramid = CpuHiZPyramid {
            mip_sizes: vec![UVec2::new(4, 4)],
            offsets: vec![0],
            values: vec![
                0.9, 0.8, 0.7, 0.6, //
                0.9, 0.8, 0.7, 0.6, //
                0.9, 0.8, 0.7, 0.6, //
                0.9, 0.8, 0.7, 0.6, //
            ],
        };

        assert!((pyramid.min_depth(0, 0, 0, 2, 2) - 0.8).abs() < 1.0e-6);
        assert!((pyramid.min_depth(0, 2, 0, 4, 4) - 0.6).abs() < 1.0e-6);
    }

    #[test]
    fn camera_reuse_rejects_zoom_projection_changes() {
        let settings = HiZOcclusionConfig::default();
        let previous_camera =
            Camera::from_config(Vec3::ZERO, -Vec3::Z, 16.0 / 9.0, &CameraConfig::default());
        let zoom_camera = Camera::from_config(
            Vec3::ZERO,
            -Vec3::Z,
            16.0 / 9.0,
            &CameraConfig { fov_y_degrees: 30.0, ..CameraConfig::default() },
        );

        assert!(!can_reuse_previous_camera(&settings, &zoom_camera, &previous_camera));
    }
}
