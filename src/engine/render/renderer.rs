mod draw;
mod mesh_upload;
mod overlay;
mod pipelines;

use std::{cmp::Ordering, num::NonZeroU64, sync::Arc};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::{
    config::{ClearColorConfig, OverlayConfig, RenderConfig},
    engine::{
        core::types::CHUNK_SIZE_I32,
        render::{
            camera::{Camera, CameraUniform},
            frustum::Frustum,
            gpu_types::{
                BaseQuadVertex, DebugOverlayInput, DebugViewMode, DrawMeta, DrawRef,
                GpuDrawIndirect, RenderBucket, RenderSettingsUniform, RenderStats,
                TextGlyphInstance, TextOverlayUniform,
            },
            hiz::HiZOcclusion,
            materials::{Materials, TextureRegistry},
            mesh_pool::{ChunkGpuEntry, MeshPool},
            meshing::{MeshingFocus, ThreadedMesher},
            targets::DepthTarget,
        },
        world::{block::resolved::ResolvedBlockRegistry, coord::ChunkCoord, storage::World},
    },
};

use self::{
    draw::{build_draw_ref_bytes, next_face_capacity, transparent_batch_center},
    pipelines::{
        create_overlay_pipeline, create_screen_tint_pipeline, create_voxel_pipeline,
        create_wireframe_pipeline,
    },
};

pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,

    pub depth_target: DepthTarget,

    pub mesh_pool: MeshPool,
    pub gpu_entries: ahash::AHashMap<ChunkCoord, ChunkGpuEntry>,

    pub draw_meta_buffer: wgpu::Buffer,
    pub draw_ref_buffer: wgpu::Buffer,
    pub draw_ref_stride: u32,
    pub indirect_buffer: wgpu::Buffer,
    pub camera_buffer: wgpu::Buffer,
    pub render_settings_buffer: wgpu::Buffer,
    pub overlay_uniform_buffer: wgpu::Buffer,
    pub overlay_instance_buffer: wgpu::Buffer,
    pub base_quad_buffer: wgpu::Buffer,
    pub base_line_buffer: wgpu::Buffer,

    pub scene_bind_group_layout: wgpu::BindGroupLayout,
    pub camera_bind_group: wgpu::BindGroup,
    pub scene_bind_group: wgpu::BindGroup,
    pub material_bind_group: wgpu::BindGroup,
    pub overlay_bind_group: wgpu::BindGroup,

    pub opaque_pipeline: wgpu::RenderPipeline,
    pub transparent_pipeline: wgpu::RenderPipeline,
    pub wireframe_pipeline: wgpu::RenderPipeline,
    pub overlay_pipeline: wgpu::RenderPipeline,
    pub underwater_tint_pipeline: wgpu::RenderPipeline,
    pub hiz_occlusion: Option<HiZOcclusion>,
    pub mesher: ThreadedMesher,
    pub resolved_blocks: ResolvedBlockRegistry,
    pub _materials: Materials,
    pub debug_view_mode: DebugViewMode,
    pub debug_overlay: Option<DebugOverlayInput>,
    pub last_frame_stats: RenderStats,
    pub use_multi_draw_indirect: bool,
    underwater_tint_active: bool,
    meshing_enqueue_budget: usize,
    mesh_upload_budget: usize,
    max_visible_draws: usize,
    overlay_config: OverlayConfig,
    clear_color: ClearColorConfig,
    opaque_draw_scratch: Vec<DrawMeta>,
    transparent_draw_scratch: Vec<(f32, DrawMeta)>,
    staged_draw_scratch: Vec<DrawMeta>,
    indirect_draw_scratch: Vec<GpuDrawIndirect>,
    drawn_chunk_scratch: ahash::AHashSet<[i32; 3]>,
}

fn non_zero_u64(value: u64, label: &str) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap_or_else(|| panic!("{label} must be non-zero"))
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        resolved_blocks: ResolvedBlockRegistry,
        texture_registry: &TextureRegistry,
        render_config: &RenderConfig,
        meshing_worker_count: usize,
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

        let adapter_features = adapter.features();
        let downlevel = adapter.get_downlevel_capabilities();
        let supports_indirect_first_instance = adapter_features
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
            && downlevel.flags.contains(
                wgpu::DownlevelFlags::VERTEX_AND_INSTANCE_INDEX_RESPECTS_RESPECTIVE_FIRST_VALUE_IN_INDIRECT_DRAW,
            );
        let use_multi_draw_indirect = supports_indirect_first_instance
            && adapter_features.contains(wgpu::Features::MULTI_DRAW_INDIRECT);
        let mut required_features = wgpu::Features::empty();

        if supports_indirect_first_instance {
            required_features |= wgpu::Features::INDIRECT_FIRST_INSTANCE;
        }
        if use_multi_draw_indirect {
            required_features |= wgpu::Features::MULTI_DRAW_INDIRECT;
        }

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

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        let depth_target =
            DepthTarget::create(&device, surface_config.width, surface_config.height);
        let mesh_pool = MeshPool::new(&device, render_config.initial_face_capacity);
        let hiz_occlusion = render_config.enable_hiz_occlusion.then(|| {
            HiZOcclusion::new(
                &device,
                surface_config.width,
                surface_config.height,
                render_config.hiz,
            )
        });
        let draw_meta_size = std::mem::size_of::<DrawMeta>() as u32;
        let draw_ref_size = std::mem::size_of::<DrawRef>() as u32;
        let draw_ref_alignment = device.limits().min_uniform_buffer_offset_alignment;
        let draw_ref_stride = draw_ref_size.div_ceil(draw_ref_alignment) * draw_ref_alignment;
        let max_visible_draws = render_config.max_visible_draws;

        let draw_meta_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draw_meta_buffer"),
            size: draw_meta_size as u64 * max_visible_draws as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let draw_ref_bytes = build_draw_ref_bytes(max_visible_draws, draw_ref_stride as usize);
        let draw_ref_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("draw_ref_buffer"),
            contents: &draw_ref_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let indirect_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("indirect_buffer"),
            size: (max_visible_draws * std::mem::size_of::<GpuDrawIndirect>()) as u64,
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
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
            contents: bytemuck::bytes_of(&RenderSettingsUniform::new(DebugViewMode::Shaded, 0)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let overlay_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay_uniform_buffer"),
            contents: bytemuck::bytes_of(&TextOverlayUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let overlay_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay_instance_buffer"),
            size: (render_config.overlay.max_glyphs * std::mem::size_of::<TextGlyphInstance>())
                as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
        let base_line = [
            BaseQuadVertex { uv: [0.0, 0.0] },
            BaseQuadVertex { uv: [1.0, 0.0] },
            BaseQuadVertex { uv: [1.0, 1.0] },
            BaseQuadVertex { uv: [0.0, 1.0] },
            BaseQuadVertex { uv: [0.0, 0.0] },
        ];
        let base_line_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("base_line_buffer"),
            contents: bytemuck::cast_slice(&base_line),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let materials = Materials::from_texture_registry(&device, &queue, texture_registry)?;
        let mesher = ThreadedMesher::new(resolved_blocks.clone(), meshing_worker_count);

        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(non_zero_u64(
                            std::mem::size_of::<CameraUniform>() as u64,
                            "camera uniform size",
                        )),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(non_zero_u64(
                            std::mem::size_of::<RenderSettingsUniform>() as u64,
                            "render settings uniform size",
                        )),
                    },
                    count: None,
                },
            ],
        });

        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: Some(non_zero_u64(
                                draw_ref_size as u64,
                                "draw reference uniform size",
                            )),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: Some(non_zero_u64(
                                draw_meta_size as u64,
                                "draw metadata buffer size",
                            )),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
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

        let overlay_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(non_zero_u64(
                        std::mem::size_of::<TextOverlayUniform>() as u64,
                        "text overlay uniform size",
                    )),
                },
                count: None,
            }],
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
            layout: &scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &draw_ref_buffer,
                        offset: 0,
                        size: Some(non_zero_u64(
                            draw_ref_size as u64,
                            "draw reference binding size",
                        )),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: draw_meta_buffer.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 2,
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

        let overlay_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay_bg"),
            layout: &overlay_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: overlay_uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("text_overlay.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: &[&camera_bgl, &scene_bind_group_layout, &material_bgl],
            push_constant_ranges: &[],
        });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("overlay_pipeline_layout"),
                bind_group_layouts: &[&overlay_bgl],
                push_constant_ranges: &[],
            });
        let screen_tint_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("screen_tint_pipeline_layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

        let opaque_pipeline = create_voxel_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            surface_config.format,
            depth_target.format,
            Some(wgpu::BlendState::REPLACE),
            true,
            "voxel_opaque_pipeline",
        );

        let transparent_pipeline = create_voxel_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            surface_config.format,
            depth_target.format,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            "voxel_transparent_pipeline",
        );
        let wireframe_pipeline = create_wireframe_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            surface_config.format,
            depth_target.format,
        );
        let overlay_pipeline = create_overlay_pipeline(
            &device,
            &overlay_pipeline_layout,
            &overlay_shader,
            surface_config.format,
            depth_target.format,
        );
        let underwater_tint_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("underwater_tint_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("underwater_tint.wgsl").into()),
        });
        let underwater_tint_pipeline = create_screen_tint_pipeline(
            &device,
            &screen_tint_pipeline_layout,
            &underwater_tint_shader,
            surface_config.format,
            depth_target.format,
        );

        if use_multi_draw_indirect {
            crate::log_info!("Using multi_draw_indirect renderer path");
        } else {
            crate::log_info!("Using looped draw renderer fallback");
        }

        Ok(Self {
            surface,
            device,
            queue,
            surface_config,
            depth_target,
            mesh_pool,
            gpu_entries: ahash::AHashMap::new(),
            draw_meta_buffer,
            draw_ref_buffer,
            draw_ref_stride,
            indirect_buffer,
            camera_buffer,
            render_settings_buffer,
            overlay_uniform_buffer,
            overlay_instance_buffer,
            base_quad_buffer,
            base_line_buffer,
            scene_bind_group_layout,
            camera_bind_group,
            scene_bind_group,
            material_bind_group,
            overlay_bind_group,
            opaque_pipeline,
            transparent_pipeline,
            wireframe_pipeline,
            overlay_pipeline,
            underwater_tint_pipeline,
            hiz_occlusion,
            mesher,
            resolved_blocks,
            _materials: materials,
            debug_view_mode: DebugViewMode::Shaded,
            debug_overlay: None,
            last_frame_stats: RenderStats {
                hiz_enabled: render_config.enable_hiz_occlusion,
                ..Default::default()
            },
            use_multi_draw_indirect,
            underwater_tint_active: false,
            meshing_enqueue_budget: render_config.meshing_enqueue_budget,
            mesh_upload_budget: render_config.mesh_upload_budget,
            max_visible_draws,
            overlay_config: render_config.overlay,
            clear_color: render_config.clear_color,
            opaque_draw_scratch: Vec::new(),
            transparent_draw_scratch: Vec::new(),
            staged_draw_scratch: Vec::new(),
            indirect_draw_scratch: Vec::new(),
            drawn_chunk_scratch: ahash::AHashSet::new(),
        })
    }

    pub fn set_debug_view_mode(&mut self, mode: DebugViewMode) {
        self.debug_view_mode = mode;
    }

    pub fn set_debug_overlay(&mut self, overlay: Option<DebugOverlayInput>) {
        self.debug_overlay = overlay;
    }

    pub fn set_underwater_tint_active(&mut self, active: bool) {
        self.underwater_tint_active = active;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width.max(1);
        self.surface_config.height = height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_target = DepthTarget::create(
            &self.device,
            self.surface_config.width,
            self.surface_config.height,
        );
        if let Some(hiz_occlusion) = &mut self.hiz_occlusion {
            hiz_occlusion.resize(
                &self.device,
                self.surface_config.width,
                self.surface_config.height,
            );
        }
    }

    pub fn pump_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::pump_meshing");

        for result in self.mesher.try_take_ready_limit(self.mesh_upload_budget) {
            self.upload_chunk_mesh(result.coord, result.mesh)?;
        }
        self.mesher.enqueue_dirty(world, focus, self.meshing_enqueue_budget)?;

        Ok(())
    }

    pub fn finish_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::finish_meshing");

        self.mesher.enqueue_dirty(world, focus, 0)?;

        while self.mesher.has_pending_work() {
            if self.mesher.has_inflight_jobs() {
                let result = self.mesher.recv_ready()?;
                self.upload_chunk_mesh(result.coord, result.mesh)?;
            }
            self.mesher.enqueue_dirty(world, focus, 0)?;
        }

        Ok(())
    }

    pub fn remove_chunk_mesh(&mut self, coord: ChunkCoord) {
        self.mesher.cancel(coord);

        if let Some(entry) = self.gpu_entries.remove(&coord) {
            self.free_gpu_entry(&entry);
        }
    }

    pub fn render(&mut self, camera: &Camera) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::render");

        let mut opaque_draws = std::mem::take(&mut self.opaque_draw_scratch);
        opaque_draws.clear();
        let mut transparent_draws = std::mem::take(&mut self.transparent_draw_scratch);
        transparent_draws.clear();
        let mut staged_draws = std::mem::take(&mut self.staged_draw_scratch);
        staged_draws.clear();
        let mut indirect_draws = std::mem::take(&mut self.indirect_draw_scratch);
        indirect_draws.clear();
        let mut drawn_chunk_origins = std::mem::take(&mut self.drawn_chunk_scratch);
        drawn_chunk_origins.clear();

        if let Some(hiz_occlusion) = &mut self.hiz_occlusion {
            hiz_occlusion.update_readback(&self.device);
        }
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&camera.build_uniform()),
        );
        self.queue.write_buffer(
            &self.render_settings_buffer,
            0,
            bytemuck::bytes_of(&RenderSettingsUniform::new(
                self.debug_view_mode,
                u32::from(
                    self.use_multi_draw_indirect
                        && self.debug_view_mode != DebugViewMode::Wireframe,
                ),
            )),
        );

        let frustum = Frustum::from_camera(camera);
        let mut frustum_culled_chunks = 0u32;
        let mut occlusion_culled_chunks = 0u32;
        for (&coord, entry) in &self.gpu_entries {
            let origin = coord.world_origin();
            let min = origin.as_vec3();
            let max = (origin + glam::IVec3::splat(CHUNK_SIZE_I32)).as_vec3();

            if !frustum.test_aabb(min, max) {
                frustum_culled_chunks += 1;
                continue;
            }

            if self
                .hiz_occlusion
                .as_ref()
                .is_some_and(|hiz_occlusion| hiz_occlusion.is_chunk_occluded(camera, min, max))
            {
                occlusion_culled_chunks += 1;
                continue;
            }

            for dir in 0..6usize {
                if let Some(slice) = entry.faces[RenderBucket::Opaque as usize][dir]
                    && slice.count > 0
                {
                    opaque_draws.push(DrawMeta {
                        chunk_origin: [origin.x, origin.y, origin.z, 0],
                        face_dir: dir as u32,
                        face_offset: slice.offset,
                        face_count: slice.count,
                        draw_id: 0,
                    });
                }
                if let Some(slice) = entry.faces[RenderBucket::Transparent as usize][dir]
                    && slice.count > 0
                {
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

        opaque_draws.sort_by_key(|draw| {
            (draw.chunk_origin[0], draw.chunk_origin[1], draw.chunk_origin[2], draw.face_dir)
        });
        transparent_draws.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(Ordering::Equal));

        if opaque_draws.len() > self.max_visible_draws {
            opaque_draws.truncate(self.max_visible_draws);
            transparent_draws.clear();
        } else {
            transparent_draws.truncate(self.max_visible_draws - opaque_draws.len());
        }

        let opaque_count = opaque_draws.len();
        staged_draws.append(&mut opaque_draws);
        staged_draws.extend(transparent_draws.drain(..).map(|(_, draw)| draw));
        for draw in &staged_draws {
            drawn_chunk_origins.insert([
                draw.chunk_origin[0],
                draw.chunk_origin[1],
                draw.chunk_origin[2],
            ]);
        }
        let drawn_chunks = drawn_chunk_origins.len() as u32;

        for (draw_id, draw) in staged_draws.iter_mut().enumerate() {
            draw.draw_id = draw_id as u32;
        }

        if !staged_draws.is_empty() {
            self.queue.write_buffer(&self.draw_meta_buffer, 0, bytemuck::cast_slice(&staged_draws));
        }
        let use_indirect_draws =
            self.use_multi_draw_indirect && self.debug_view_mode != DebugViewMode::Wireframe;

        if use_indirect_draws && !staged_draws.is_empty() {
            indirect_draws.extend(staged_draws.iter().enumerate().map(|(draw_index, draw)| {
                GpuDrawIndirect::for_draw(draw_index as u32, draw.face_count)
            }));
            self.queue.write_buffer(
                &self.indirect_buffer,
                0,
                bytemuck::cast_slice(&indirect_draws),
            );
        }

        let current_stats = RenderStats {
            gpu_chunks: self.gpu_entries.len() as u32,
            drawn_chunks,
            frustum_culled_chunks,
            occlusion_culled_chunks,
            directional_culled_draws: 0,
            opaque_draws: opaque_count as u32,
            transparent_draws: (staged_draws.len() - opaque_count) as u32,
            meshing_pending_chunks: self.mesher.pending_count() as u32,
            hiz_enabled: self.hiz_occlusion.is_some(),
        };
        self.last_frame_stats = current_stats;

        let overlay_instances = self.build_overlay_instances(current_stats);
        self.queue.write_buffer(
            &self.overlay_uniform_buffer,
            0,
            bytemuck::bytes_of(&TextOverlayUniform {
                screen_size: [self.surface_config.width as f32, self.surface_config.height as f32],
                _pad: [0.0; 2],
            }),
        );
        if !overlay_instances.is_empty() {
            self.queue.write_buffer(
                &self.overlay_instance_buffer,
                0,
                bytemuck::cast_slice(&overlay_instances),
            );
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
                            r: self.clear_color.r,
                            g: self.clear_color.g,
                            b: self.clear_color.b,
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
            if self.debug_view_mode == DebugViewMode::Wireframe {
                pass.set_vertex_buffer(0, self.base_line_buffer.slice(..));
                pass.set_pipeline(&self.wireframe_pipeline);

                for (i, draw) in staged_draws.iter().enumerate() {
                    let dynamic_offset = i as u32 * self.draw_ref_stride;
                    pass.set_bind_group(1, &self.scene_bind_group, &[dynamic_offset]);
                    pass.draw(0..5, 0..draw.face_count);
                }
            } else {
                pass.set_vertex_buffer(0, self.base_quad_buffer.slice(..));

                pass.set_pipeline(&self.opaque_pipeline);
                if use_indirect_draws {
                    if opaque_count > 0 {
                        pass.set_bind_group(1, &self.scene_bind_group, &[0]);
                        pass.multi_draw_indirect(&self.indirect_buffer, 0, opaque_count as u32);
                    }
                } else {
                    for (i, draw) in staged_draws.iter().take(opaque_count).enumerate() {
                        let dynamic_offset = i as u32 * self.draw_ref_stride;
                        pass.set_bind_group(1, &self.scene_bind_group, &[dynamic_offset]);
                        pass.draw(0..4, 0..draw.face_count);
                    }
                }

                if opaque_count < staged_draws.len() {
                    pass.set_pipeline(&self.transparent_pipeline);
                    if use_indirect_draws {
                        pass.set_bind_group(1, &self.scene_bind_group, &[0]);
                        pass.multi_draw_indirect(
                            &self.indirect_buffer,
                            (opaque_count * std::mem::size_of::<GpuDrawIndirect>()) as u64,
                            (staged_draws.len() - opaque_count) as u32,
                        );
                    } else {
                        for (i, draw) in staged_draws.iter().enumerate().skip(opaque_count) {
                            let dynamic_offset = i as u32 * self.draw_ref_stride;
                            pass.set_bind_group(1, &self.scene_bind_group, &[dynamic_offset]);
                            pass.draw(0..4, 0..draw.face_count);
                        }
                    }
                }
            }

            if self.underwater_tint_active {
                pass.set_pipeline(&self.underwater_tint_pipeline);
                pass.set_vertex_buffer(0, self.base_quad_buffer.slice(..));
                pass.draw(0..4, 0..1);
            }

            if !overlay_instances.is_empty() {
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, &self.overlay_bind_group, &[]);
                pass.set_vertex_buffer(0, self.overlay_instance_buffer.slice(..));
                pass.draw(0..4, 0..overlay_instances.len() as u32);
            }
        }

        let hiz_readback_slot = self.hiz_occlusion.as_mut().and_then(|hiz_occlusion| {
            hiz_occlusion.record(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.depth_target,
                self.surface_config.width,
                self.surface_config.height,
            )
        });

        self.queue.submit(Some(encoder.finish()));
        if let (Some(hiz_occlusion), Some(slot_index)) =
            (&mut self.hiz_occlusion, hiz_readback_slot)
        {
            hiz_occlusion.start_readback(slot_index);
        }
        if let Some(hiz_occlusion) = &mut self.hiz_occlusion {
            hiz_occlusion.finish_frame(camera);
        }
        frame.present();

        self.opaque_draw_scratch = opaque_draws;
        self.transparent_draw_scratch = transparent_draws;
        self.staged_draw_scratch = staged_draws;
        self.indirect_draw_scratch = indirect_draws;
        self.drawn_chunk_scratch = drawn_chunk_origins;

        Ok(())
    }
}
