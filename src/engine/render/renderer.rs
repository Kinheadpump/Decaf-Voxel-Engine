use std::{cmp::Ordering, num::NonZeroU64, sync::Arc};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::engine::{
    core::types::{CHUNK_SIZE_I32, INITIAL_FACE_CAPACITY, MAX_VISIBLE_DRAWS},
    render::{
        camera::{Camera, CameraUniform},
        frustum::Frustum,
        gpu_types::{
            BaseQuadVertex, ChunkMeshCpu, DebugViewMode, DrawMeta, PackedFace, RenderBucket,
            RenderSettingsUniform,
        },
        materials::{Materials, TextureRegistry},
        mesh_pool::{ChunkGpuEntry, GpuSlice, MeshPool},
        meshing::ThreadedMesher,
        targets::DepthTarget,
    },
    world::{block::resolved::ResolvedBlockRegistry, coord::ChunkCoord, storage::World},
};

pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,

    pub depth_target: DepthTarget,

    pub mesh_pool: MeshPool,
    pub gpu_entries: ahash::AHashMap<ChunkCoord, ChunkGpuEntry>,

    pub draw_meta_buffer: wgpu::Buffer,
    pub draw_meta_stride: u32,
    pub camera_buffer: wgpu::Buffer,
    pub render_settings_buffer: wgpu::Buffer,
    pub base_quad_buffer: wgpu::Buffer,

    pub camera_bind_group: wgpu::BindGroup,
    pub scene_bind_group: wgpu::BindGroup,
    pub material_bind_group: wgpu::BindGroup,

    pub opaque_pipeline: wgpu::RenderPipeline,
    pub transparent_pipeline: wgpu::RenderPipeline,
    pub mesher: ThreadedMesher,
    pub resolved_blocks: ResolvedBlockRegistry,
    pub materials: Materials,
    pub debug_view_mode: DebugViewMode,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        resolved_blocks: ResolvedBlockRegistry,
        texture_registry: &TextureRegistry,
    ) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let size = window.inner_size();
        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("no suitable GPU adapter found"))?;

        let required_features = wgpu::Features::empty();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("device"),
                    required_features,
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let depth_target = DepthTarget::create(&device, config.width, config.height);
        let mesh_pool = MeshPool::new(&device, INITIAL_FACE_CAPACITY as u32);
        let draw_meta_size = std::mem::size_of::<DrawMeta>() as u32;
        let draw_meta_alignment = device.limits().min_uniform_buffer_offset_alignment;
        let draw_meta_stride = ((draw_meta_size + draw_meta_alignment - 1) / draw_meta_alignment)
            * draw_meta_alignment;

        let draw_meta_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draw_meta_buffer"),
            size: draw_meta_stride as u64 * MAX_VISIBLE_DRAWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera_buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let render_settings_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("render_settings_buffer"),
            contents: bytemuck::bytes_of(&RenderSettingsUniform::new(DebugViewMode::Shaded)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let base_quad = [
            BaseQuadVertex { uv: [0.0, 0.0] },
            BaseQuadVertex { uv: [1.0, 0.0] },
            BaseQuadVertex { uv: [0.0, 1.0] },
            BaseQuadVertex { uv: [1.0, 1.0] },
        ];

        let base_quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("base_quad_buffer"),
            contents: bytemuck::cast_slice(&base_quad),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let materials = Materials::from_texture_registry(&device, &queue, texture_registry)?;
        let mesher = ThreadedMesher::new(resolved_blocks.clone());

        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(
                            NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64).unwrap(),
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(
                            NonZeroU64::new(std::mem::size_of::<RenderSettingsUniform>() as u64)
                                .unwrap(),
                        ),
                    },
                    count: None,
                },
            ],
        });

        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(NonZeroU64::new(draw_meta_size as u64).unwrap()),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let material_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bg"),
            layout: &camera_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: render_settings_buffer.as_entire_binding(),
                },
            ],
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bg"),
            layout: &scene_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &draw_meta_buffer,
                        offset: 0,
                        size: Some(NonZeroU64::new(draw_meta_size as u64).unwrap()),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: mesh_pool.face_buffer.as_entire_binding(),
                },
            ],
        });

        let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material_bg"),
            layout: &material_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&materials.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&materials.sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: &[&camera_bgl, &scene_bgl, &material_bgl],
            push_constant_ranges: &[],
        });

        let opaque_pipeline = create_voxel_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            depth_target.format,
            Some(wgpu::BlendState::REPLACE),
            true,
            "voxel_opaque_pipeline",
        );

        let transparent_pipeline = create_voxel_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            depth_target.format,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            "voxel_transparent_pipeline",
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth_target,
            mesh_pool,
            gpu_entries: ahash::AHashMap::new(),
            draw_meta_buffer,
            draw_meta_stride,
            camera_buffer,
            render_settings_buffer,
            base_quad_buffer,
            camera_bind_group,
            scene_bind_group,
            material_bind_group,
            opaque_pipeline,
            transparent_pipeline,
            mesher,
            resolved_blocks,
            materials,
            debug_view_mode: DebugViewMode::Shaded,
        })
    }

    pub fn set_debug_view_mode(&mut self, mode: DebugViewMode) {
        self.debug_view_mode = mode;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_target =
            DepthTarget::create(&self.device, self.config.width, self.config.height);
    }

    pub fn pump_meshing(&mut self, world: &mut World) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::pump_meshing");

        self.mesher.enqueue_dirty(world)?;

        for result in self.mesher.try_take_ready() {
            self.upload_chunk_mesh(result.coord, result.mesh)?;
        }

        Ok(())
    }

    pub fn finish_meshing(&mut self, world: &mut World) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::finish_meshing");

        self.pump_meshing(world)?;

        while self.mesher.has_pending() {
            let result = self.mesher.recv_ready()?;
            self.upload_chunk_mesh(result.coord, result.mesh)?;
            self.mesher.enqueue_dirty(world)?;
        }

        Ok(())
    }

    fn upload_chunk_mesh(&mut self, coord: ChunkCoord, mesh: ChunkMeshCpu) -> anyhow::Result<()> {
        if let Some(old_entry) = self.gpu_entries.remove(&coord) {
            for bucket in RenderBucket::ALL {
                for maybe in old_entry.faces[bucket as usize].into_iter().flatten() {
                    self.mesh_pool.free(maybe.offset, maybe.count);
                }
            }
        }

        let mut new_entry = ChunkGpuEntry::default();

        for bucket in RenderBucket::ALL {
            for dir in 0..6usize {
                let faces = &mesh.faces[bucket as usize][dir];
                if faces.is_empty() {
                    continue;
                }

                let count = faces.len() as u32;
                let offset = self.mesh_pool.alloc(count).ok_or_else(|| {
                    anyhow::anyhow!(
                        "face buffer exhausted (capacity {} faces)",
                        self.mesh_pool.capacity_faces
                    )
                })?;

                self.queue.write_buffer(
                    &self.mesh_pool.face_buffer,
                    offset as u64 * std::mem::size_of::<PackedFace>() as u64,
                    bytemuck::cast_slice(faces),
                );

                new_entry.faces[bucket as usize][dir] = Some(GpuSlice { offset, count });
            }
        }

        self.gpu_entries.insert(coord, new_entry);
        Ok(())
    }

    pub fn render(&mut self, camera: &Camera) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::render");

        let _ = self.materials.keep_alive();
        let _ = self.depth_target.keep_alive();
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&camera.build_uniform()),
        );
        self.queue.write_buffer(
            &self.render_settings_buffer,
            0,
            bytemuck::bytes_of(&RenderSettingsUniform::new(self.debug_view_mode)),
        );

        let frustum = Frustum::from_camera(camera);
        let mut opaque_draws = Vec::<DrawMeta>::with_capacity(4096);
        let mut transparent_draws = Vec::<(f32, DrawMeta)>::with_capacity(2048);

        for (&coord, entry) in &self.gpu_entries {
            let origin = coord.world_origin();
            let min = origin.as_vec3();
            let max = (origin + glam::IVec3::splat(CHUNK_SIZE_I32)).as_vec3();

            if !frustum.test_aabb(min, max) {
                continue;
            }

            for dir in 0..6usize {
                if let Some(slice) = entry.faces[RenderBucket::Opaque as usize][dir] {
                    if slice.count > 0 {
                        opaque_draws.push(DrawMeta {
                            chunk_origin: [origin.x, origin.y, origin.z, 0],
                            face_dir: dir as u32,
                            face_offset: slice.offset,
                            face_count: slice.count,
                            draw_id: 0,
                        });
                    }
                }
            }

            for dir in 0..6usize {
                if let Some(slice) = entry.faces[RenderBucket::Transparent as usize][dir] {
                    if slice.count > 0 {
                        let batch_center = transparent_batch_center(origin, dir as u32);
                        let distance_sq = (batch_center - camera.position).length_squared();
                        transparent_draws.push((
                            distance_sq,
                            DrawMeta {
                                chunk_origin: [origin.x, origin.y, origin.z, 0],
                                face_dir: dir as u32,
                                face_offset: slice.offset,
                                face_count: slice.count,
                                draw_id: 0,
                            },
                        ));
                    }
                }
            }
        }

        opaque_draws.sort_by_key(|draw| {
            (draw.chunk_origin[0], draw.chunk_origin[1], draw.chunk_origin[2], draw.face_dir)
        });
        transparent_draws.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(Ordering::Equal));

        if opaque_draws.len() > MAX_VISIBLE_DRAWS {
            opaque_draws.truncate(MAX_VISIBLE_DRAWS);
            transparent_draws.clear();
        } else {
            transparent_draws.truncate(MAX_VISIBLE_DRAWS - opaque_draws.len());
        }

        let opaque_count = opaque_draws.len();
        let mut staged_draws = Vec::with_capacity(opaque_count + transparent_draws.len());
        staged_draws.extend(opaque_draws);
        staged_draws.extend(transparent_draws.into_iter().map(|(_, draw)| draw));

        for (draw_id, draw) in staged_draws.iter_mut().enumerate() {
            draw.draw_id = draw_id as u32;
        }

        let mut draw_meta_bytes = vec![0u8; staged_draws.len() * self.draw_meta_stride as usize];
        let draw_meta_size = std::mem::size_of::<DrawMeta>();

        for (i, draw) in staged_draws.iter().enumerate() {
            let offset = i * self.draw_meta_stride as usize;
            draw_meta_bytes[offset..offset + draw_meta_size]
                .copy_from_slice(bytemuck::bytes_of(draw));
        }

        if !draw_meta_bytes.is_empty() {
            self.queue.write_buffer(&self.draw_meta_buffer, 0, &draw_meta_bytes);
        }

        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("main_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.45,
                            g: 0.70,
                            b: 0.95,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_target.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(2, &self.material_bind_group, &[]);
            pass.set_vertex_buffer(0, self.base_quad_buffer.slice(..));

            pass.set_pipeline(&self.opaque_pipeline);
            for (i, draw) in staged_draws.iter().take(opaque_count).enumerate() {
                let dynamic_offset = i as u32 * self.draw_meta_stride;
                pass.set_bind_group(1, &self.scene_bind_group, &[dynamic_offset]);
                pass.draw(0..4, 0..draw.face_count);
            }

            if opaque_count < staged_draws.len() {
                pass.set_pipeline(&self.transparent_pipeline);
                for (i, draw) in staged_draws.iter().enumerate().skip(opaque_count) {
                    let dynamic_offset = i as u32 * self.draw_meta_stride;
                    pass.set_bind_group(1, &self.scene_bind_group, &[dynamic_offset]);
                    pass.draw(0..4, 0..draw.face_count);
                }
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

fn create_voxel_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    depth_write_enabled: bool,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<BaseQuadVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled,
            depth_compare: wgpu::CompareFunction::Greater,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn transparent_batch_center(origin: glam::IVec3, face_dir: u32) -> glam::Vec3 {
    let min = origin.as_vec3();
    let half = CHUNK_SIZE_I32 as f32 * 0.5;
    let center = min + glam::Vec3::splat(half);

    match face_dir {
        0 => center + glam::Vec3::new(half, 0.0, 0.0),
        1 => center - glam::Vec3::new(half, 0.0, 0.0),
        2 => center + glam::Vec3::new(0.0, half, 0.0),
        3 => center - glam::Vec3::new(0.0, half, 0.0),
        4 => center + glam::Vec3::new(0.0, 0.0, half),
        _ => center - glam::Vec3::new(0.0, 0.0, half),
    }
}
