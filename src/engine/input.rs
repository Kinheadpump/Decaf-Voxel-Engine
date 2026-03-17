use std::{collections::HashSet, time::Instant};

use winit::{
    event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Debug)]
pub struct InputState {
    held_keys: HashSet<KeyCode>,
    pressed_keys: HashSet<KeyCode>,
    released_keys: HashSet<KeyCode>,

    held_mouse: HashSet<MouseButton>,
    pressed_mouse: HashSet<MouseButton>,
    released_mouse: HashSet<MouseButton>,

    pub mouse_delta: (f32, f32),
    pub cursor_grabbed: bool,
    pub last_frame_time: Instant,
    pub dt: f32,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            held_keys: HashSet::new(),
            pressed_keys: HashSet::new(),
            released_keys: HashSet::new(),
            held_mouse: HashSet::new(),
            pressed_mouse: HashSet::new(),
            released_mouse: HashSet::new(),
            mouse_delta: (0.0, 0.0),
            cursor_grabbed: false,
            last_frame_time: Instant::now(),
            dt: 0.0,
        }
    }

    pub fn begin_frame(&mut self) {
        self.pressed_keys.clear();
        self.released_keys.clear();
        self.pressed_mouse.clear();
        self.released_mouse.clear();
        self.mouse_delta = (0.0, 0.0);

        let now = Instant::now();
        self.dt = (now - self.last_frame_time).as_secs_f32().min(0.05);
        self.last_frame_time = now;
    }

    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            if self.held_keys.insert(code) {
                                self.pressed_keys.insert(code);
                            }
                        }
                        ElementState::Released => {
                            self.held_keys.remove(&code);
                            self.released_keys.insert(code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => {
                    if self.held_mouse.insert(*button) {
                        self.pressed_mouse.insert(*button);
                    }
                }
                ElementState::Released => {
                    self.held_mouse.remove(button);
                    self.released_mouse.insert(*button);
                }
            },
            _ => {}
        }
    }

    pub fn handle_device_event(&mut self, event: &DeviceEvent) {
        match event {
            DeviceEvent::MouseMotion { delta } => {
                self.mouse_delta.0 += delta.0 as f32;
                self.mouse_delta.1 += delta.1 as f32;
            }
            _ => {}
        }
    }

    #[inline]
    pub fn key_held(&self, key: KeyCode) -> bool {
        self.held_keys.contains(&key)
    }

    #[inline]
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.pressed_keys.contains(&key)
    }

    #[inline]
    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.pressed_mouse.contains(&button)
    }

    #[cfg(test)]
    pub fn set_key_held_for_test(&mut self, key: KeyCode) {
        self.held_keys.insert(key);
    }
}
