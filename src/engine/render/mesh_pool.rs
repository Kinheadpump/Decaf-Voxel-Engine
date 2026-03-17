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
        self.free.push((offset, count));
        self.free.sort_by_key(|&(o, _)| o);
        self.coalesce();
    }

    pub fn total_free_faces(&self) -> u32 {
        self.free.iter().map(|&(_, count)| count).sum()
    }

    fn coalesce(&mut self) {
        if self.free.is_empty() {
            return;
        }

        let mut out = Vec::with_capacity(self.free.len());
        let mut cur = self.free[0];

        for &(off, cnt) in &self.free[1..] {
            if cur.0 + cur.1 == off {
                cur.1 += cnt;
            } else {
                out.push(cur);
                cur = (off, cnt);
            }
        }

        out.push(cur);
        self.free = out;
    }
}
