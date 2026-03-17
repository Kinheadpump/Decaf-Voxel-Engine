const FPS_SAMPLE_WINDOW_SECONDS: f32 = 0.25;

pub struct FpsCounter {
    accumulated_time: f32,
    accumulated_frames: u32,
    displayed_fps: u32,
}

impl FpsCounter {
    pub fn new() -> Self {
        Self { accumulated_time: 0.0, accumulated_frames: 0, displayed_fps: 0 }
    }

    pub fn sample(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        self.accumulated_time += dt;
        self.accumulated_frames += 1;
        self.displayed_fps = (self.accumulated_frames as f32
            / self.accumulated_time.max(f32::EPSILON))
        .round() as u32;

        if self.accumulated_time >= FPS_SAMPLE_WINDOW_SECONDS {
            self.accumulated_time = 0.0;
            self.accumulated_frames = 0;
        }
    }

    pub fn displayed_fps(&self) -> u32 {
        self.displayed_fps
    }
}
