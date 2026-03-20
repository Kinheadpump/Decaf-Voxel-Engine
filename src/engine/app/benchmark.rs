use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use wgpu::Maintain;
use winit::{dpi::PhysicalSize, event_loop::EventLoop, window::WindowBuilder};

use crate::{
    config::{Config, MeshingUploadBenchmarkConfig},
    engine::{
        app::{
            runtime::resolve_background_worker_counts, spawn::spawn_position_near_world_origin,
            streaming::WorldStreamer,
        },
        core::math::{IVec3, Vec3},
        render::{
            materials::{create_hud_texture_registry, create_texture_registry},
            meshing::MeshingFocus,
            renderer::{MeshingPassStats, Renderer},
        },
        world::{
            biome::BiomeTable,
            block::{
                create_default_block_registry, id::BlockId, registry::BlockRegistry,
                resolved::ResolvedBlockRegistry,
            },
            coord::{ChunkCoord, LocalVoxelPos, WorldVoxelPos},
            generator::{ChunkGenerator, StagedGenerator},
            storage::World,
        },
    },
    logging,
};

const BENCHMARK_LOCAL_POSITIONS: [[u32; 3]; 8] =
    [[4, 4, 4], [27, 16, 11], [0, 8, 8], [8, 8, 31], [31, 8, 8], [8, 0, 8], [8, 31, 8], [8, 8, 0]];

pub(super) async fn run_meshing_upload(config: Config) -> anyhow::Result<()> {
    let _span = crate::profile_span!("benchmark::meshing_upload");
    let benchmark_config = sanitize_benchmark_config(config.benchmark.meshing_upload);
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Decaf Benchmark")
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(
                benchmark_config.window_width,
                benchmark_config.window_height,
            ))
            .build(&event_loop)?,
    );
    let render_config = config.render;
    let player_config = config.player;
    let world_config = config.world;
    let (generation_worker_count, meshing_worker_count) =
        resolve_background_worker_counts(&render_config);

    crate::log_info!(
        "Benchmark config: preload {}x{} chunks, edit radius {}x{}, warmup {}, measured {}, wait_for_gpu {}",
        benchmark_config.preload_radius_xz,
        benchmark_config.preload_radius_y,
        benchmark_config.edit_radius_xz,
        benchmark_config.edit_radius_y,
        benchmark_config.warmup_iterations,
        benchmark_config.measured_iterations,
        benchmark_config.wait_for_gpu
    );
    crate::log_info!(
        "Benchmark workers: generation {}, meshing {}",
        generation_worker_count,
        meshing_worker_count
    );

    let block_registry = create_default_block_registry();
    let texture_registry = create_texture_registry(&block_registry);
    let hud_texture_registry = create_hud_texture_registry(&block_registry);
    let resolved_blocks =
        ResolvedBlockRegistry::build(&block_registry, texture_registry.layer_map());
    let biomes = BiomeTable::load_from_file(&world_config.biomes_file, &block_registry)?;
    let water_block_id = block_registry.must_get_id("water");
    let staged_generator = Arc::new(StagedGenerator::new(
        world_config.seed,
        water_block_id,
        world_config.terrain,
        biomes,
    ));
    let spawn_position =
        spawn_position_near_world_origin(&staged_generator, &world_config, &player_config);
    let focus =
        MeshingFocus::new(ChunkCoord::from_world_voxel(spawn_position.floor().as_ivec3()), Vec3::Z);

    let chunk_generator: Arc<dyn ChunkGenerator> = staged_generator.clone();
    let mut streamer = WorldStreamer::new(
        chunk_generator,
        generation_worker_count,
        render_config.stream_max_inflight_generations,
    );
    let mut world = World::new();
    streamer.finish_generation(
        &mut world,
        focus,
        benchmark_config.preload_radius_xz,
        benchmark_config.preload_radius_y,
        0,
    )?;

    let mut renderer = Renderer::new(
        window.clone(),
        resolved_blocks,
        &texture_registry,
        &hud_texture_registry,
        &render_config,
        meshing_worker_count,
    )
    .await?;

    let initial_full_build =
        run_meshing_pass(&mut renderer, &mut world, focus, benchmark_config.wait_for_gpu)?;

    let mut workload = BenchmarkWorkload::new(&world, &block_registry, focus, benchmark_config)
        .context("failed to build a meshing/upload benchmark workload")?;
    workload.prepare_baseline(&mut world);
    let _baseline_prepare =
        run_meshing_pass(&mut renderer, &mut world, focus, benchmark_config.wait_for_gpu)?;

    for _ in 0..benchmark_config.warmup_iterations {
        let _ = run_benchmark_iteration(
            &mut renderer,
            &mut world,
            focus,
            &mut workload,
            benchmark_config.wait_for_gpu,
        )?;
    }

    let mut steady_state = BenchmarkAccumulator::default();
    for _ in 0..benchmark_config.measured_iterations {
        let iteration = run_benchmark_iteration(
            &mut renderer,
            &mut world,
            focus,
            &mut workload,
            benchmark_config.wait_for_gpu,
        )?;
        steady_state.record(iteration);
    }

    print_report(benchmark_config, focus, &workload, initial_full_build, steady_state.finish());

    drop(renderer);
    drop(window);
    drop(event_loop);
    Ok(())
}

fn sanitize_benchmark_config(
    mut benchmark_config: MeshingUploadBenchmarkConfig,
) -> MeshingUploadBenchmarkConfig {
    benchmark_config.warmup_iterations = benchmark_config.warmup_iterations.max(1);
    benchmark_config.measured_iterations = benchmark_config.measured_iterations.max(1);
    benchmark_config.preload_radius_xz = benchmark_config.preload_radius_xz.max(0);
    benchmark_config.preload_radius_y = benchmark_config.preload_radius_y.max(0);
    benchmark_config.edit_radius_xz =
        benchmark_config.edit_radius_xz.clamp(0, benchmark_config.preload_radius_xz);
    benchmark_config.edit_radius_y =
        benchmark_config.edit_radius_y.clamp(0, benchmark_config.preload_radius_y);
    benchmark_config.edits_per_chunk =
        benchmark_config.edits_per_chunk.clamp(1, BENCHMARK_LOCAL_POSITIONS.len());
    benchmark_config.window_width = benchmark_config.window_width.max(1);
    benchmark_config.window_height = benchmark_config.window_height.max(1);
    benchmark_config
}

#[derive(Clone, Copy, Debug)]
struct BenchmarkIteration {
    wall_cpu_time_ns: u64,
    gpu_wait_time_ns: u64,
    pass: MeshingPassStats,
}

#[derive(Clone, Copy, Debug)]
struct BenchmarkSummary {
    iterations: usize,
    wall_cpu_avg_ns: u64,
    wall_cpu_min_ns: u64,
    wall_cpu_max_ns: u64,
    gpu_wait_avg_ns: u64,
    pass_avg: MeshingPassStats,
}

#[derive(Default)]
struct BenchmarkAccumulator {
    iterations: usize,
    wall_cpu_total_ns: u128,
    wall_cpu_min_ns: u64,
    wall_cpu_max_ns: u64,
    gpu_wait_total_ns: u128,
    pass_totals: PassTotals,
}

#[derive(Default)]
struct PassTotals {
    chunk_results: u128,
    faces_uploaded: u128,
    dirty_slices: u128,
    slice_buffer_growths: u128,
    build_cpu_time_ns: u128,
    snapshot_capture_cpu_time_ns: u128,
    slice_construction_cpu_time_ns: u128,
    greedy_merge_cpu_time_ns: u128,
    flatten_cpu_time_ns: u128,
    upload_cpu_time_ns: u128,
    wait_cpu_time_ns: u128,
}

impl BenchmarkAccumulator {
    fn record(&mut self, iteration: BenchmarkIteration) {
        self.iterations += 1;
        self.wall_cpu_total_ns += u128::from(iteration.wall_cpu_time_ns);
        self.gpu_wait_total_ns += u128::from(iteration.gpu_wait_time_ns);
        if self.iterations == 1 {
            self.wall_cpu_min_ns = iteration.wall_cpu_time_ns;
            self.wall_cpu_max_ns = iteration.wall_cpu_time_ns;
        } else {
            self.wall_cpu_min_ns = self.wall_cpu_min_ns.min(iteration.wall_cpu_time_ns);
            self.wall_cpu_max_ns = self.wall_cpu_max_ns.max(iteration.wall_cpu_time_ns);
        }

        self.pass_totals.chunk_results += u128::from(iteration.pass.chunk_results);
        self.pass_totals.faces_uploaded += u128::from(iteration.pass.faces_uploaded);
        self.pass_totals.dirty_slices += u128::from(iteration.pass.dirty_slices);
        self.pass_totals.slice_buffer_growths += u128::from(iteration.pass.slice_buffer_growths);
        self.pass_totals.build_cpu_time_ns += u128::from(iteration.pass.build_cpu_time_ns);
        self.pass_totals.snapshot_capture_cpu_time_ns +=
            u128::from(iteration.pass.snapshot_capture_cpu_time_ns);
        self.pass_totals.slice_construction_cpu_time_ns +=
            u128::from(iteration.pass.slice_construction_cpu_time_ns);
        self.pass_totals.greedy_merge_cpu_time_ns +=
            u128::from(iteration.pass.greedy_merge_cpu_time_ns);
        self.pass_totals.flatten_cpu_time_ns += u128::from(iteration.pass.flatten_cpu_time_ns);
        self.pass_totals.upload_cpu_time_ns += u128::from(iteration.pass.upload_cpu_time_ns);
        self.pass_totals.wait_cpu_time_ns += u128::from(iteration.pass.wait_cpu_time_ns);
    }

    fn finish(self) -> BenchmarkSummary {
        let iterations = self.iterations.max(1) as u128;
        BenchmarkSummary {
            iterations: self.iterations.max(1),
            wall_cpu_avg_ns: average_u64(self.wall_cpu_total_ns, iterations),
            wall_cpu_min_ns: self.wall_cpu_min_ns,
            wall_cpu_max_ns: self.wall_cpu_max_ns,
            gpu_wait_avg_ns: average_u64(self.gpu_wait_total_ns, iterations),
            pass_avg: MeshingPassStats {
                chunk_results: average_u32(self.pass_totals.chunk_results, iterations),
                faces_uploaded: average_u32(self.pass_totals.faces_uploaded, iterations),
                dirty_slices: average_u32(self.pass_totals.dirty_slices, iterations),
                slice_buffer_growths: average_u32(
                    self.pass_totals.slice_buffer_growths,
                    iterations,
                ),
                build_cpu_time_ns: average_u64(self.pass_totals.build_cpu_time_ns, iterations),
                snapshot_capture_cpu_time_ns: average_u64(
                    self.pass_totals.snapshot_capture_cpu_time_ns,
                    iterations,
                ),
                slice_construction_cpu_time_ns: average_u64(
                    self.pass_totals.slice_construction_cpu_time_ns,
                    iterations,
                ),
                greedy_merge_cpu_time_ns: average_u64(
                    self.pass_totals.greedy_merge_cpu_time_ns,
                    iterations,
                ),
                flatten_cpu_time_ns: average_u64(self.pass_totals.flatten_cpu_time_ns, iterations),
                upload_cpu_time_ns: average_u64(self.pass_totals.upload_cpu_time_ns, iterations),
                wait_cpu_time_ns: average_u64(self.pass_totals.wait_cpu_time_ns, iterations),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BenchmarkEditSite {
    position: WorldVoxelPos,
}

struct BenchmarkWorkload {
    sites: Vec<BenchmarkEditSite>,
    baseline_block: BlockId,
    alternate_block: BlockId,
    next_block: BlockId,
    touched_chunk_count: usize,
}

impl BenchmarkWorkload {
    fn new(
        world: &World,
        block_registry: &BlockRegistry,
        focus: MeshingFocus,
        benchmark_config: MeshingUploadBenchmarkConfig,
    ) -> anyhow::Result<Self> {
        let baseline_block = block_registry.must_get_id("stone");
        let alternate_block = block_registry.must_get_id("oak_planks");
        let edits_per_chunk = benchmark_config.edits_per_chunk.max(1);
        let coords = benchmark_chunk_coords(
            focus.center,
            benchmark_config.edit_radius_xz,
            benchmark_config.edit_radius_y,
        );
        let mut sites = Vec::new();
        let mut touched_chunk_count = 0usize;

        for coord in coords {
            if !world.contains_chunk(coord) {
                continue;
            }

            touched_chunk_count += 1;
            for &[x, y, z] in BENCHMARK_LOCAL_POSITIONS.iter().take(edits_per_chunk) {
                let local = LocalVoxelPos::new(x, y, z);
                sites.push(BenchmarkEditSite { position: coord.world_voxel(local) });
            }
        }

        anyhow::ensure!(
            !sites.is_empty(),
            "benchmark workload did not find any loaded chunks to edit"
        );

        Ok(Self {
            sites,
            baseline_block,
            alternate_block,
            next_block: alternate_block,
            touched_chunk_count,
        })
    }

    fn site_count(&self) -> usize {
        self.sites.len()
    }

    fn chunk_count(&self) -> usize {
        self.touched_chunk_count
    }

    fn prepare_baseline(&self, world: &mut World) {
        for site in &self.sites {
            let _ = world.set_block_world(site.position, self.baseline_block);
        }
    }

    fn apply_next(&mut self, world: &mut World) {
        for site in &self.sites {
            let _ = world.set_block_world(site.position, self.next_block);
        }

        self.next_block = if self.next_block == self.alternate_block {
            self.baseline_block
        } else {
            self.alternate_block
        };
    }
}

fn benchmark_chunk_coords(center: ChunkCoord, radius_xz: i32, radius_y: i32) -> Vec<ChunkCoord> {
    let mut coords = Vec::new();
    for chunk_z in -radius_xz..=radius_xz {
        for chunk_y in -radius_y..=radius_y {
            for chunk_x in -radius_xz..=radius_xz {
                coords.push(center.offset(IVec3::new(chunk_x, chunk_y, chunk_z)));
            }
        }
    }
    coords
}

fn run_benchmark_iteration(
    renderer: &mut Renderer,
    world: &mut World,
    focus: MeshingFocus,
    workload: &mut BenchmarkWorkload,
    wait_for_gpu: bool,
) -> anyhow::Result<BenchmarkIteration> {
    let _span = crate::profile_span!("benchmark::meshing_upload_iteration");
    workload.apply_next(world);
    run_meshing_pass(renderer, world, focus, wait_for_gpu)
}

fn run_meshing_pass(
    renderer: &mut Renderer,
    world: &mut World,
    focus: MeshingFocus,
    wait_for_gpu: bool,
) -> anyhow::Result<BenchmarkIteration> {
    let pass_started_at = Instant::now();
    renderer.finish_meshing(world, focus)?;
    let wall_cpu_time_ns = duration_to_nanos(pass_started_at.elapsed());
    let pass = renderer.last_meshing_pass_stats();
    let gpu_wait_time_ns = if wait_for_gpu {
        let wait_started_at = Instant::now();
        let _ = renderer.device.poll(Maintain::Wait);
        duration_to_nanos(wait_started_at.elapsed())
    } else {
        0
    };
    logging::frame_mark();

    Ok(BenchmarkIteration { wall_cpu_time_ns, gpu_wait_time_ns, pass })
}

fn print_report(
    benchmark_config: MeshingUploadBenchmarkConfig,
    focus: MeshingFocus,
    workload: &BenchmarkWorkload,
    initial_full_build: BenchmarkIteration,
    steady_state: BenchmarkSummary,
) {
    crate::log_info!("Meshing/upload benchmark focus chunk: {:?}", focus.center.0);
    crate::log_info!(
        "Workload: {} chunks, {} edit sites, toggle {} <-> {}",
        workload.chunk_count(),
        workload.site_count(),
        "stone",
        "oak_planks"
    );
    crate::log_info!(
        "Initial full build: wall {:.3} ms | chunks {} | dirty slices {} | faces {} | build sum {:.3} ms | upload {:.3} ms | wait {:.3} ms{}",
        nanos_to_ms(initial_full_build.wall_cpu_time_ns),
        initial_full_build.pass.chunk_results,
        initial_full_build.pass.dirty_slices,
        initial_full_build.pass.faces_uploaded,
        nanos_to_ms(initial_full_build.pass.build_cpu_time_ns),
        nanos_to_ms(initial_full_build.pass.upload_cpu_time_ns),
        nanos_to_ms(initial_full_build.pass.wait_cpu_time_ns),
        format_gpu_wait(initial_full_build.gpu_wait_time_ns, benchmark_config.wait_for_gpu),
    );
    log_stage_breakdown("Initial stage breakdown", initial_full_build.pass);
    crate::log_info!(
        "Steady state over {} iterations: wall avg {:.3} ms (min {:.3}, max {:.3}){}",
        steady_state.iterations,
        nanos_to_ms(steady_state.wall_cpu_avg_ns),
        nanos_to_ms(steady_state.wall_cpu_min_ns),
        nanos_to_ms(steady_state.wall_cpu_max_ns),
        format_gpu_wait(steady_state.gpu_wait_avg_ns, benchmark_config.wait_for_gpu),
    );
    crate::log_info!(
        "Steady-state averages: chunks {} | dirty slices {} | faces {} | build sum {:.3} ms | upload {:.3} ms | wait {:.3} ms | growths {}",
        steady_state.pass_avg.chunk_results,
        steady_state.pass_avg.dirty_slices,
        steady_state.pass_avg.faces_uploaded,
        nanos_to_ms(steady_state.pass_avg.build_cpu_time_ns),
        nanos_to_ms(steady_state.pass_avg.upload_cpu_time_ns),
        nanos_to_ms(steady_state.pass_avg.wait_cpu_time_ns),
        steady_state.pass_avg.slice_buffer_growths,
    );
    log_stage_breakdown("Steady-state stage averages", steady_state.pass_avg);
    crate::log_info!(
        "Analysis: {}",
        classify_bottleneck(steady_state, benchmark_config.wait_for_gpu)
    );
}

fn log_stage_breakdown(label: &str, pass: MeshingPassStats) {
    crate::log_info!(
        "{label}: snapshot {:.3} ms | mask/slice {:.3} ms | greedy {:.3} ms | flatten {:.3} ms",
        nanos_to_ms(pass.snapshot_capture_cpu_time_ns),
        nanos_to_ms(pass.slice_construction_cpu_time_ns),
        nanos_to_ms(pass.greedy_merge_cpu_time_ns),
        nanos_to_ms(pass.flatten_cpu_time_ns),
    );
}

fn classify_bottleneck(summary: BenchmarkSummary, wait_for_gpu: bool) -> String {
    let wall = summary.wall_cpu_avg_ns.max(1);
    let upload_share = summary.pass_avg.upload_cpu_time_ns as f64 / wall as f64;
    let wait_share = summary.pass_avg.wait_cpu_time_ns as f64 / wall as f64;
    let gpu_wait_share = summary.gpu_wait_avg_ns as f64 / wall as f64;
    let dominant_stage = dominant_meshing_stage(summary.pass_avg);

    if wait_for_gpu && gpu_wait_share >= 0.50 {
        "GPU completion time is a major end-to-end cost for this workload.".to_string()
    } else if upload_share >= 0.45 {
        "CPU-side mesh upload is taking a large share of wall time and is the first place to investigate."
            .to_string()
    } else if wait_share >= 0.25 {
        format!(
            "The main thread is spending noticeable time waiting on meshing workers, so chunk meshing is the current wall-time bottleneck. Inside meshing, {dominant_stage} dominates."
        )
    } else if summary.pass_avg.build_cpu_time_ns > summary.pass_avg.upload_cpu_time_ns * 2 {
        format!(
            "Meshing consumes more total CPU than upload, but worker parallelism is hiding part of that cost. Inside meshing, {dominant_stage} dominates."
        )
    } else {
        "Meshing and upload are fairly balanced in this benchmark; profile both before picking the next optimization."
            .to_string()
    }
}

fn dominant_meshing_stage(pass: MeshingPassStats) -> &'static str {
    let mut dominant = ("snapshot capture", pass.snapshot_capture_cpu_time_ns);

    for candidate in [
        ("mask/slice construction", pass.slice_construction_cpu_time_ns),
        ("greedy merging", pass.greedy_merge_cpu_time_ns),
        ("flattening", pass.flatten_cpu_time_ns),
    ] {
        if candidate.1 > dominant.1 {
            dominant = candidate;
        }
    }

    dominant.0
}

fn format_gpu_wait(gpu_wait_time_ns: u64, wait_for_gpu: bool) -> String {
    if wait_for_gpu {
        format!(" | gpu wait {:.3} ms", nanos_to_ms(gpu_wait_time_ns))
    } else {
        String::new()
    }
}

#[inline]
fn average_u32(total: u128, count: u128) -> u32 {
    (total / count).min(u128::from(u32::MAX)) as u32
}

#[inline]
fn average_u64(total: u128, count: u128) -> u64 {
    (total / count).min(u128::from(u64::MAX)) as u64
}

#[inline]
fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

#[inline]
fn nanos_to_ms(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottleneck_classifier_flags_upload_dominance() {
        let summary = BenchmarkSummary {
            iterations: 4,
            wall_cpu_avg_ns: 10_000_000,
            wall_cpu_min_ns: 9_000_000,
            wall_cpu_max_ns: 11_000_000,
            gpu_wait_avg_ns: 0,
            pass_avg: MeshingPassStats {
                chunk_results: 8,
                faces_uploaded: 16_000,
                dirty_slices: 256,
                slice_buffer_growths: 0,
                build_cpu_time_ns: 4_000_000,
                snapshot_capture_cpu_time_ns: 100_000,
                slice_construction_cpu_time_ns: 2_300_000,
                greedy_merge_cpu_time_ns: 1_200_000,
                flatten_cpu_time_ns: 400_000,
                upload_cpu_time_ns: 5_500_000,
                wait_cpu_time_ns: 500_000,
            },
        };

        assert!(classify_bottleneck(summary, false).contains("upload"));
    }

    #[test]
    fn bottleneck_classifier_flags_meshing_wait() {
        let summary = BenchmarkSummary {
            iterations: 4,
            wall_cpu_avg_ns: 10_000_000,
            wall_cpu_min_ns: 9_000_000,
            wall_cpu_max_ns: 11_000_000,
            gpu_wait_avg_ns: 0,
            pass_avg: MeshingPassStats {
                chunk_results: 8,
                faces_uploaded: 16_000,
                dirty_slices: 256,
                slice_buffer_growths: 0,
                build_cpu_time_ns: 12_000_000,
                snapshot_capture_cpu_time_ns: 150_000,
                slice_construction_cpu_time_ns: 7_000_000,
                greedy_merge_cpu_time_ns: 4_200_000,
                flatten_cpu_time_ns: 650_000,
                upload_cpu_time_ns: 1_000_000,
                wait_cpu_time_ns: 3_500_000,
            },
        };

        assert!(classify_bottleneck(summary, false).contains("meshing"));
    }
}
