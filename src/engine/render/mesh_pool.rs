use crate::engine::render::gpu_types::PackedFace;

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuSlice {
    pub offset: u32,
    pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ChunkGpuEntry {
    pub faces: [[Option<GpuSlice>; 6]; 2],
}

pub struct MeshPool {
    pub face_buffer: wgpu::Buffer,
    pub capacity_faces: u32,
    free: Vec<(u32, u32)>, // (offset, count), sorted by offset
}

impl MeshPool {
    pub fn new(device: &wgpu::Device, capacity_faces: u32) -> Self {
        let face_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("face_buffer"),
            size: capacity_faces as u64 * std::mem::size_of::<PackedFace>() as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self { face_buffer, capacity_faces, free: vec![(0, capacity_faces)] }
    }

    pub fn alloc(&mut self, count: u32) -> Option<u32> {
        let idx = self.free.iter().position(|&(_, c)| c >= count)?;
        let (offset, size) = self.free[idx];

        if size == count {
            self.free.remove(idx);
        } else {
            self.free[idx] = (offset + count, size - count);
        }
        Some(offset)
    }

    pub fn free(&mut self, offset: u32, count: u32) {
        let insert_at = self
            .free
            .binary_search_by_key(&offset, |&(existing_offset, _)| existing_offset)
            .unwrap_or_else(|index| index);
        self.free.insert(insert_at, (offset, count));

        let mut merged_index = insert_at;
        if merged_index > 0 {
            let previous = self.free[merged_index - 1];
            if previous.0 + previous.1 == self.free[merged_index].0 {
                self.free[merged_index - 1].1 += self.free[merged_index].1;
                self.free.remove(merged_index);
                merged_index -= 1;
            }
        }

        if merged_index + 1 < self.free.len() {
            let next = self.free[merged_index + 1];
            if self.free[merged_index].0 + self.free[merged_index].1 == next.0 {
                self.free[merged_index].1 += next.1;
                self.free.remove(merged_index + 1);
            }
        }
    }

    pub fn total_free_faces(&self) -> u32 {
        self.free.iter().map(|&(_, count)| count).sum()
    }
}
