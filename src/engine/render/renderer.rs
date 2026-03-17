use std::{cmp::Ordering, num::NonZeroU64, sync::Arc};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::engine::{
    core::types::{CHUNK_SIZE_I32, INITIAL_FACE_CAPACITY, MAX_VISIBLE_DRAWS},
    render::{
        camera::{Camera, CameraUniform},
        frustum::Frustum,
        gpu_types::{
            BaseQuadVertex, ChunkMeshCpu, DebugOverlayInput, DebugViewMode, DrawMeta, PackedFace,
            RenderBucket, RenderSettingsUniform, RenderStats, TextGlyphInstance,
            TextOverlayUniform,
        },
        hiz::HiZOcclusion,
        materials::{Materials, TextureRegistry},
        mesh_pool::{ChunkGpuEntry, GpuSlice, MeshPool},
        meshing::{MeshingFocus, ThreadedMesher},
        targets::DepthTarget,
    },
    world::{block::resolved::ResolvedBlockRegistry, coord::ChunkCoord, storage::World},
};

const MAX_OVERLAY_GLYPHS: usize = 256;
const OVERLAY_GLYPH_SIZE: [f32; 2] = [12.0, 15.0];
const OVERLAY_PADDING_PX: [f32; 2] = [8.0, 8.0];
const OVERLAY_GLYPH_ADVANCE_X: f32 = 13.0;
const OVERLAY_LINE_ADVANCE_Y: f32 = 18.0;

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
    pub overlay_uniform_buffer: wgpu::Buffer,
    pub overlay_instance_buffer: wgpu::Buffer,
    pub base_quad_buffer: wgpu::Buffer,

    pub scene_bind_group_layout: wgpu::BindGroupLayout,
    pub camera_bind_group: wgpu::BindGroup,
    pub scene_bind_group: wgpu::BindGroup,
    pub material_bind_group: wgpu::BindGroup,
    pub overlay_bind_group: wgpu::BindGroup,

    pub opaque_pipeline: wgpu::RenderPipeline,
    pub transparent_pipeline: wgpu::RenderPipeline,
    pub overlay_pipeline: wgpu::RenderPipeline,
    pub hiz_occlusion: Option<HiZOcclusion>,
    pub mesher: ThreadedMesher,
    pub resolved_blocks: ResolvedBlockRegistry,
    pub materials: Materials,
    pub debug_view_mode: DebugViewMode,
    pub debug_overlay: Option<DebugOverlayInput>,
    pub last_frame_stats: RenderStats,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        resolved_blocks: ResolvedBlockRegistry,
        texture_registry: &TextureRegistry,
        enable_hiz_occlusion: bool,
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
        let hiz_occlusion =
            enable_hiz_occlusion.then(|| HiZOcclusion::new(&device, config.width, config.height));
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

        let overlay_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay_uniform_buffer"),
            contents: bytemuck::bytes_of(&TextOverlayUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let overlay_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay_instance_buffer"),
            size: (MAX_OVERLAY_GLYPHS * std::mem::size_of::<TextGlyphInstance>()) as u64,
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

        let overlay_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(
                        NonZeroU64::new(std::mem::size_of::<TextOverlayUniform>() as u64).unwrap(),
                    ),
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
        let overlay_pipeline = create_overlay_pipeline(
            &device,
            &overlay_pipeline_layout,
            &overlay_shader,
            config.format,
            depth_target.format,
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
            overlay_uniform_buffer,
            overlay_instance_buffer,
            base_quad_buffer,
            scene_bind_group_layout,
            camera_bind_group,
            scene_bind_group,
            material_bind_group,
            overlay_bind_group,
            opaque_pipeline,
            transparent_pipeline,
            overlay_pipeline,
            hiz_occlusion,
            mesher,
            resolved_blocks,
            materials,
            debug_view_mode: DebugViewMode::Shaded,
            debug_overlay: None,
            last_frame_stats: RenderStats {
                hiz_enabled: enable_hiz_occlusion,
                ..Default::default()
            },
        })
    }

    pub fn set_debug_view_mode(&mut self, mode: DebugViewMode) {
        self.debug_view_mode = mode;
    }

    pub fn set_debug_overlay(&mut self, overlay: Option<DebugOverlayInput>) {
        self.debug_overlay = overlay;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_target =
            DepthTarget::create(&self.device, self.config.width, self.config.height);
        if let Some(hiz_occlusion) = &mut self.hiz_occlusion {
            hiz_occlusion.resize(&self.device, self.config.width, self.config.height);
        }
    }

    pub fn pump_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::pump_meshing");

        self.mesher.enqueue_dirty(world, focus)?;

        for result in self.mesher.try_take_ready() {
            self.upload_chunk_mesh(result.coord, result.mesh)?;
        }

        Ok(())
    }

    pub fn finish_meshing(&mut self, world: &mut World, focus: MeshingFocus) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::finish_meshing");

        self.pump_meshing(world, focus)?;

        while self.mesher.has_pending() {
            let result = self.mesher.recv_ready()?;
            self.upload_chunk_mesh(result.coord, result.mesh)?;
            self.mesher.enqueue_dirty(world, focus)?;
        }

        Ok(())
    }

    pub fn remove_chunk_mesh(&mut self, coord: ChunkCoord) {
        self.mesher.cancel(coord);

        if let Some(entry) = self.gpu_entries.remove(&coord) {
            self.free_gpu_entry(&entry);
        }
    }

    fn upload_chunk_mesh(&mut self, coord: ChunkCoord, mesh: ChunkMeshCpu) -> anyhow::Result<()> {
        if let Some(old_entry) = self.gpu_entries.remove(&coord) {
            self.free_gpu_entry(&old_entry);
        }

        let required_faces = mesh.face_count();
        let mut new_entry = if let Some(new_entry) = self.try_allocate_mesh_entry(&mesh)? {
            new_entry
        } else {
            self.ensure_mesh_capacity(self.live_face_count() + required_faces)?;
            self.try_allocate_mesh_entry(&mesh)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "face buffer exhausted after repack (capacity {} faces, required {})",
                    self.mesh_pool.capacity_faces,
                    self.live_face_count() + required_faces
                )
            })?
        };

        self.gpu_entries.insert(coord, std::mem::take(&mut new_entry));
        Ok(())
    }

    fn try_allocate_mesh_entry(
        &mut self,
        mesh: &ChunkMeshCpu,
    ) -> anyhow::Result<Option<ChunkGpuEntry>> {
        let mut new_entry = ChunkGpuEntry::default();
        let mut allocated = Vec::new();

        for bucket in RenderBucket::ALL {
            for dir in 0..6usize {
                let faces = &mesh.faces[bucket as usize][dir];
                if faces.is_empty() {
                    continue;
                }

                let count = faces.len() as u32;
                let Some(offset) = self.mesh_pool.alloc(count) else {
                    for (offset, count) in allocated.drain(..) {
                        self.mesh_pool.free(offset, count);
                    }
                    return Ok(None);
                };

                self.queue.write_buffer(
                    &self.mesh_pool.face_buffer,
                    offset as u64 * std::mem::size_of::<PackedFace>() as u64,
                    bytemuck::cast_slice(faces),
                );

                new_entry.faces[bucket as usize][dir] = Some(GpuSlice { offset, count });
                allocated.push((offset, count));
            }
        }

        Ok(Some(new_entry))
    }

    fn free_gpu_entry(&mut self, entry: &ChunkGpuEntry) {
        for bucket in RenderBucket::ALL {
            for maybe in entry.faces[bucket as usize].into_iter().flatten() {
                self.mesh_pool.free(maybe.offset, maybe.count);
            }
        }
    }

    fn live_face_count(&self) -> u32 {
        self.gpu_entries
            .values()
            .flat_map(|entry| entry.faces.iter())
            .flat_map(|dirs| dirs.iter())
            .flatten()
            .map(|slice| slice.count)
            .sum()
    }

    fn ensure_mesh_capacity(&mut self, required_faces: u32) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::ensure_mesh_capacity");
        let current_capacity = self.mesh_pool.capacity_faces;
        let target_capacity = next_face_capacity(current_capacity, required_faces);

        if target_capacity > current_capacity {
            crate::log_info!(
                "Growing face buffer from {} to {} faces",
                current_capacity,
                target_capacity
            );
        } else {
            crate::log_debug!(
                "Repacking face buffer at {} faces ({} faces free) to recover contiguous space",
                current_capacity,
                self.mesh_pool.total_free_faces()
            );
        }

        self.repack_mesh_pool(target_capacity)
    }

    fn repack_mesh_pool(&mut self, target_capacity: u32) -> anyhow::Result<()> {
        let live_faces = self.live_face_count();
        anyhow::ensure!(
            target_capacity >= live_faces,
            "repack target capacity {} is smaller than {} live faces",
            target_capacity,
            live_faces
        );

        let old_buffer = &self.mesh_pool.face_buffer;
        let mut new_pool = MeshPool::new(&self.device, target_capacity);
        let mut new_entries = ahash::AHashMap::with_capacity(self.gpu_entries.len());
        let mut coords: Vec<_> = self.gpu_entries.keys().copied().collect();
        coords.sort_by_key(|coord| (coord.0.x, coord.0.y, coord.0.z));

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mesh_pool_repack_encoder"),
        });
        let face_size = std::mem::size_of::<PackedFace>() as u64;

        for coord in coords {
            let old_entry = self.gpu_entries.get(&coord).cloned().ok_or_else(|| {
                anyhow::anyhow!("missing GPU entry for chunk during mesh pool repack")
            })?;
            let mut new_entry = ChunkGpuEntry::default();

            for bucket in RenderBucket::ALL {
                for dir in 0..6usize {
                    let Some(old_slice) = old_entry.faces[bucket as usize][dir] else {
                        continue;
                    };

                    let new_offset = new_pool.alloc(old_slice.count).ok_or_else(|| {
                        anyhow::anyhow!(
                            "mesh pool repack ran out of space at capacity {} faces",
                            target_capacity
                        )
                    })?;

                    encoder.copy_buffer_to_buffer(
                        old_buffer,
                        old_slice.offset as u64 * face_size,
                        &new_pool.face_buffer,
                        new_offset as u64 * face_size,
                        old_slice.count as u64 * face_size,
                    );

                    new_entry.faces[bucket as usize][dir] =
                        Some(GpuSlice { offset: new_offset, count: old_slice.count });
                }
            }

            new_entries.insert(coord, new_entry);
        }

        self.queue.submit(Some(encoder.finish()));
        self.mesh_pool = new_pool;
        self.gpu_entries = new_entries;
        self.rebuild_scene_bind_group();
        Ok(())
    }

    fn rebuild_scene_bind_group(&mut self) {
        let draw_meta_size = std::mem::size_of::<DrawMeta>() as u64;

        self.scene_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bg"),
            layout: &self.scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.draw_meta_buffer,
                        offset: 0,
                        size: Some(NonZeroU64::new(draw_meta_size).unwrap()),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.mesh_pool.face_buffer.as_entire_binding(),
                },
            ],
        });
    }

    fn build_overlay_instances(&self, stats: RenderStats) -> Vec<TextGlyphInstance> {
        let Some(debug_overlay) = self.debug_overlay else {
            return Vec::new();
        };

        let mode_label = self.debug_view_mode.label().to_ascii_uppercase();
        let hiz_label = if stats.hiz_enabled { "ON" } else { "OFF" };
        let lines = [
            format!("FPS: {}", debug_overlay.fps),
            format!(
                "POS: {} {} {}",
                debug_overlay.player_voxel[0],
                debug_overlay.player_voxel[1],
                debug_overlay.player_voxel[2]
            ),
            format!(
                "CHK: {} {} {}",
                debug_overlay.player_chunk[0],
                debug_overlay.player_chunk[1],
                debug_overlay.player_chunk[2]
            ),
            format!("FACING: {}", debug_overlay.player_facing),
            format!("WORLD: {}", stats.loaded_chunks),
            format!("GPU: {}", stats.gpu_chunks),
            format!("DRAWN: {}", stats.drawn_chunks),
            format!("FRUSTUM: {}", stats.frustum_culled_chunks),
            format!("OCCLUDED: {}", stats.occlusion_culled_chunks),
            format!("DIR: {}", stats.directional_culled_draws),
            format!("OPAQUE: {}", stats.opaque_draws),
            format!("TRANS: {}", stats.transparent_draws),
            format!("MESH: {}", stats.meshing_pending_chunks),
            format!("HIZ: {hiz_label}"),
            format!("MODE: {mode_label}"),
        ];
        let mut instances = Vec::with_capacity(MAX_OVERLAY_GLYPHS.min(lines.len() * 16));

        'lines: for (line_index, line) in lines.iter().enumerate() {
            for (column_index, ch) in line.chars().enumerate() {
                if instances.len() >= MAX_OVERLAY_GLYPHS {
                    break 'lines;
                }

                let ch = ch.to_ascii_uppercase();
                if !overlay_supports_char(ch) {
                    continue;
                }

                instances.push(TextGlyphInstance {
                    origin_px: [
                        OVERLAY_PADDING_PX[0] + column_index as f32 * OVERLAY_GLYPH_ADVANCE_X,
                        OVERLAY_PADDING_PX[1] + line_index as f32 * OVERLAY_LINE_ADVANCE_Y,
                    ],
                    size_px: OVERLAY_GLYPH_SIZE,
                    glyph_code: ch as u32,
                    _pad: [0; 3],
                });
            }
        }

        instances
    }

    pub fn render(&mut self, camera: &Camera) -> anyhow::Result<()> {
        let _span = crate::profile_span!("renderer::render");

        let _ = self.materials.keep_alive();
        let _ = self.depth_target.keep_alive();
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
            bytemuck::bytes_of(&RenderSettingsUniform::new(self.debug_view_mode)),
        );

        let frustum = Frustum::from_camera(camera);
        let mut opaque_draws = Vec::<DrawMeta>::with_capacity(4096);
        let mut transparent_draws = Vec::<(f32, DrawMeta)>::with_capacity(2048);
        let mut frustum_culled_chunks = 0u32;
        let mut occlusion_culled_chunks = 0u32;
        let mut directional_culled_draws = 0u32;

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
                let faces_camera =
                    chunk_face_can_face_camera(camera.position, min, max, dir as u32);
                if let Some(slice) = entry.faces[RenderBucket::Opaque as usize][dir] {
                    if slice.count > 0 {
                        if !faces_camera {
                            directional_culled_draws += 1;
                            continue;
                        }
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
                let faces_camera =
                    chunk_face_can_face_camera(camera.position, min, max, dir as u32);
                if let Some(slice) = entry.faces[RenderBucket::Transparent as usize][dir] {
                    if slice.count > 0 {
                        if !faces_camera {
                            directional_culled_draws += 1;
                            continue;
                        }
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
        let drawn_chunks = staged_draws
            .iter()
            .map(|draw| [draw.chunk_origin[0], draw.chunk_origin[1], draw.chunk_origin[2]])
            .collect::<ahash::AHashSet<_>>()
            .len() as u32;

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

        let current_stats = RenderStats {
            loaded_chunks: self
                .debug_overlay
                .map(|overlay| overlay.loaded_chunks)
                .unwrap_or(self.gpu_entries.len() as u32),
            gpu_chunks: self.gpu_entries.len() as u32,
            drawn_chunks,
            frustum_culled_chunks,
            occlusion_culled_chunks,
            directional_culled_draws,
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
                screen_size: [self.config.width as f32, self.config.height as f32],
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
                self.config.width,
                self.config.height,
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

fn create_overlay_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("overlay_pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<TextGlyphInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn overlay_supports_char(ch: char) -> bool {
    ch == ' ' || ch == ':' || ch == '-' || ch.is_ascii_digit() || ch.is_ascii_uppercase()
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

fn chunk_face_can_face_camera(
    camera_position: glam::Vec3,
    chunk_min: glam::Vec3,
    chunk_max: glam::Vec3,
    face_dir: u32,
) -> bool {
    const FACE_VISIBILITY_EPSILON: f32 = 1.0e-4;
    const FACE_OFFSET: f32 = 1.0;

    match face_dir {
        0 => camera_position.x > chunk_min.x + FACE_OFFSET + FACE_VISIBILITY_EPSILON,
        1 => camera_position.x < chunk_max.x - FACE_OFFSET - FACE_VISIBILITY_EPSILON,
        2 => camera_position.y > chunk_min.y + FACE_OFFSET + FACE_VISIBILITY_EPSILON,
        3 => camera_position.y < chunk_max.y - FACE_OFFSET - FACE_VISIBILITY_EPSILON,
        4 => camera_position.z > chunk_min.z + FACE_OFFSET + FACE_VISIBILITY_EPSILON,
        _ => camera_position.z < chunk_max.z - FACE_OFFSET - FACE_VISIBILITY_EPSILON,
    }
}

fn next_face_capacity(current_capacity: u32, required_faces: u32) -> u32 {
    if current_capacity == 0 {
        return required_faces.max(1);
    }

    let mut capacity = current_capacity;

    while capacity < required_faces {
        capacity = capacity.saturating_mul(2);
        if capacity == u32::MAX {
            break;
        }
    }

    capacity.max(required_faces)
}

#[cfg(test)]
mod tests {
    use super::{chunk_face_can_face_camera, next_face_capacity};

    #[test]
    fn capacity_stays_when_requirement_fits() {
        assert_eq!(next_face_capacity(1024, 768), 1024);
    }

    #[test]
    fn capacity_grows_by_doubling_until_requirement_fits() {
        assert_eq!(next_face_capacity(1024, 1500), 2048);
    }

    #[test]
    fn zero_capacity_grows_to_requirement() {
        assert_eq!(next_face_capacity(0, 300), 300);
    }

    #[test]
    fn directional_pruning_skips_backside_batches() {
        let chunk_min = glam::Vec3::ZERO;
        let chunk_max = glam::Vec3::splat(32.0);

        assert!(!chunk_face_can_face_camera(
            glam::Vec3::new(-10.0, 10.0, 10.0),
            chunk_min,
            chunk_max,
            0,
        ));
        assert!(chunk_face_can_face_camera(
            glam::Vec3::new(-10.0, 10.0, 10.0),
            chunk_min,
            chunk_max,
            1,
        ));
    }

    #[test]
    fn directional_pruning_keeps_both_sides_when_camera_is_inside_chunk() {
        let chunk_min = glam::Vec3::ZERO;
        let chunk_max = glam::Vec3::splat(32.0);
        let camera = glam::Vec3::splat(16.0);

        assert!(chunk_face_can_face_camera(camera, chunk_min, chunk_max, 0));
        assert!(chunk_face_can_face_camera(camera, chunk_min, chunk_max, 1));
        assert!(chunk_face_can_face_camera(camera, chunk_min, chunk_max, 2));
        assert!(chunk_face_can_face_camera(camera, chunk_min, chunk_max, 3));
    }
}
