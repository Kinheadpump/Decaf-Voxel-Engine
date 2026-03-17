use std::sync::Arc;
use winit::{
    dpi::PhysicalSize,
    event::*,
    event_loop::EventLoop,
    window::WindowBuilder,
};


use crate::engine::{
    core::{types::{WINDOW_HEIGHT, WINDOW_WIDTH}, math::{Vec3, IVec3}},
    render::{camera::Camera, renderer::Renderer},
    world::{
        generator::{ChunkGenerator, FlatGenerator},
        storage::World,
        coord::ChunkCoord,
        chunk::Chunk,
    },
};

pub async fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let window = Arc::new(WindowBuilder::new()
        .with_title("Decaf")
        .with_inner_size(PhysicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .build(&event_loop)?);

    let mut world = World::new();
    let generator = FlatGenerator;

    for cz in -4..=4 {
        for cy in -1..=1 {
            for cx in -4..=4 {
                let coord = ChunkCoord(IVec3::new(cx, cy, cz));
                let mut chunk = Chunk::new();
                generator.generate(coord, &mut chunk);
                world.chunks.insert(coord, chunk);
                world.mark_dirty(coord);
            }
        }
    }

    let mut renderer = Renderer::new(window.clone()).await?;

    let mut camera = Camera::new(
        Vec3::new(32.0, 40.0, 80.0),
        Vec3::new(0.0, 0.0, -1.0),
        WINDOW_WIDTH as f32 / WINDOW_HEIGHT as f32,
    );

    renderer.rebuild_dirty_meshes(&mut world)?;
    window.request_redraw();

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(size) => {
                    renderer.resize(size.width.max(1), size.height.max(1));
                    camera.aspect = size.width as f32 / size.height.max(1) as f32;
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    renderer.render(&camera).unwrap();
                }
                _ => {}
            },
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    })?;

    #[allow(unreachable_code)]
    Ok(())
}
