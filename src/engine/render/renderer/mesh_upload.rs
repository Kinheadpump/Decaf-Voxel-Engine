use crate::engine::{
    render::{
        gpu_types::{ChunkMeshCpu, DrawRef, PackedFace, RenderBucket},
        mesh_pool::{ChunkGpuEntry, GpuSlice, MeshPool},
    },
    world::coord::ChunkCoord,
};

use super::{Renderer, next_face_capacity, non_zero_u64};

impl Renderer {
    pub(super) fn upload_chunk_mesh(
        &mut self,
        coord: ChunkCoord,
        mesh: ChunkMeshCpu,
    ) -> anyhow::Result<()> {
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

    pub(super) fn free_gpu_entry(&mut self, entry: &ChunkGpuEntry) {
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
        let draw_ref_size = std::mem::size_of::<DrawRef>() as u64;

        self.scene_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bg"),
            layout: &self.scene_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.draw_ref_buffer,
                        offset: 0,
                        size: Some(non_zero_u64(draw_ref_size, "draw reference binding size")),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.draw_meta_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.mesh_pool.face_buffer.as_entire_binding(),
                },
            ],
        });
    }
}
