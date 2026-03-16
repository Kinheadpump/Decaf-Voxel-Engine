# `SPRINTS.md`

> Sprint plan for the voxel engine project.  
> Assumption: 2 developers, Rust, performance-focused, CPU meshing, GPU rendering.  
> Structure: each sprint has goals, deliverables, technical tasks, success criteria, and risks.  
> These sprints are ordered so that you get a working engine early, then harden architecture, then optimize.

---

# Sprint 0 — Project Setup and Technical Foundations

## Goals
- Set up the repository and development workflow
- Define coding standards and architecture boundaries
- Create the base application loop and tooling
- Make performance instrumentation a first-class part of development

## Deliverables
- Working Rust workspace
- Window opens and closes cleanly
- Render loop skeleton
- Logging/tracing/profiling integrated
- Initial module layout created
- Design decisions documented

## Tasks

### Project structure
- Create Cargo workspace
- Create crates/modules:
  - `app`
  - `core`
  - `world`
  - `generation`
  - `meshing`
  - `lighting`
  - `streaming`
  - `render`
  - `jobs`
  - `debug`

### Tech setup
- Add dependencies:
  - `winit`
  - `wgpu`
  - `glam`
  - `rayon`
  - `serde`
  - `tracing`
  - `smallvec`
  - `bytemuck`
  - `hashbrown`

### Tooling
- Add:
  - logging via `tracing`
  - Tracy integration or equivalent profiling hooks
  - debug config loading
- Add clippy/rustfmt settings
- Add basic CI for formatting and build checks

### Application shell
- Window creation
- Main loop
- Input handling
- Camera movement in empty world
- Frame timing counters

### Documentation
- Finalize:
  - chunk coordinate conventions
  - axis directions
  - naming conventions
  - chunk state machine draft

## Success criteria
- Project builds on all target dev machines
- Camera can move in an empty rendered scene
- Profiling zones can be added and seen
- Base architecture compiles and is agreed upon

## Risks
- Overbuilding infrastructure before any world rendering
- Spending too much time on “perfect” architecture

## Notes
Keep this sprint short. The point is to establish velocity, not polish tooling forever.

---

# Sprint 1 — Minimal World Representation and Basic Rendering

## Goals
- Implement the first world data structures
- Add chunk coordinate math
- Create a simple chunk manager
- Render visible voxel geometry with basic face culling

## Deliverables
- Chunk coordinate system
- Dense chunk storage
- Flat test world or simple noise world
- Naive visible-face meshing
- Rendered chunks around player
- Basic frustum culling

## Tasks

### World coordinates
- Implement:
  - world block coordinates
  - chunk coordinates
  - local voxel coordinates
- Add conversion helpers:
  - world -> chunk/local
  - chunk/local -> world

### Chunk storage
- Create initial dense chunk/subchunk format
- Choose compile-time chunk size abstraction supporting:
  - 16³
  - 32³
- Store:
  - block IDs in flat array
- Add read/write APIs

### Block registry
- Add static block definitions:
  - air
  - stone
  - dirt
  - grass
- Properties:
  - opaque
  - solid
  - texture/material id

### Chunk manager
- Add chunk map keyed by chunk coord
- Add basic chunk lifetime:
  - requested
  - resident
- Keep initial implementation simple

### Meshing
- Implement first mesher:
  - visible-face culling only
  - no greedy merge yet
- Include neighbor face checks

### Rendering
- Create GPU vertex/index buffers for chunk meshes
- Basic shader for textured or colored cubes
- Per-subchunk mesh rendering
- AABB frustum culling

## Success criteria
- Player can move through a rendered voxel terrain
- Chunks appear/disappear around player
- No visible face leaks at chunk borders
- Frustum culling works

## Risks
- Border bugs between chunks
- Chunk coordinate conversion mistakes
- Too much time spent on rendering polish

---

# Sprint 2 — Streaming Pipeline and Async Chunk Lifecycle

## Goals
- Introduce proper chunk streaming around player
- Build an asynchronous chunk pipeline
- Separate generation, meshing, and upload stages
- Add versioning and job completion handling

## Deliverables
- Chunk state machine
- Async generation jobs
- Async meshing jobs
- Upload queue
- Residency radius around player
- Prioritized chunk requests

## Tasks

### Chunk state machine
- Implement states:
  - unloaded
  - requested
  - generating
  - meshing_pending
  - meshing
  - upload_pending
  - resident
  - unloading

### Job system
- Use Rayon for worker execution
- Add world-managed queues:
  - generation queue
  - meshing queue
  - upload queue

### Versioning
- Add:
  - data version
  - mesh version
- Reject stale completed jobs

### Residency management
- Add render radius
- Add prefetch radius
- Request chunks as player moves
- Unload far chunks

### Streaming behavior
- Sort requested chunks by priority:
  - distance
  - direction of movement
- Add cancellation/ignore stale output behavior

## Success criteria
- World streams continuously as player moves
- No main-thread stalls from generation or meshing
- Old task results cannot corrupt new chunk state
- Chunk load/unload is stable under fast movement

## Risks
- Race conditions in chunk state transitions
- Incorrect stale-task acceptance
- Too much locking in chunk manager

---

# Sprint 3 — Deterministic Terrain Generation v1

## Goals
- Replace test terrain with deterministic seeded terrain
- Introduce proper biome map generation
- Build a first natural terrain pipeline
- Make generation order-independent

## Deliverables
- Global seed support
- Deterministic chunk generation
- Biome map system
- Height/density terrain generation
- Surface block placement rules

## Tasks

### Seed system
- Add world seed
- Add deterministic hash utilities:
  - seed + chunk coord
  - seed + region coord + feature type

### Biomes
- Generate low-frequency fields:
  - temperature
  - humidity
  - continentalness
- Map these to biome IDs
- Allow blending/weights later

### Terrain generation
- Implement first density-based terrain
- Use layered noise:
  - large-scale base terrain
  - medium-scale hills/ridges
  - local detail
- Generate stone/air result
- Then apply surface rules:
  - grass
  - dirt
  - sand
  - snow later

### Validation
- Ensure terrain is seamless across chunk borders
- Ensure same seed always yields same blocks

## Success criteria
- The world looks natural enough to navigate
- Same seed always produces identical results
- Terrain is seamless across chunk boundaries
- Biomes visibly affect terrain composition

## Risks
- Terrain feels too noisy or artificial
- Biome transitions feel harsh
- Generation too slow due to expensive noise evaluation

---

# Sprint 4 — Cave Generation and Better Biome Shaping

## Goals
- Add cave systems
- Improve terrain variation
- Expand biome logic
- Move closer to a world that feels game-ready

## Deliverables
- 3D cave generation
- Improved biome-dependent terrain parameters
- Overhangs/cliffs where appropriate
- Better world variety

## Tasks

### Caves
- Add 3D noise-based cave density subtraction
- Implement cave styles:
  - cheese caves
  - spaghetti/tunnel caves
- Add domain warping to reduce artificial repetition

### Biome shaping
- Give each biome terrain parameters:
  - elevation bias
  - roughness
  - cave frequency
  - surface block set
- Blend terrain parameters across biome boundaries

### Surface rules refinement
- Add biome-specific top layers
- Improve shoreline handling
- Add snowline or altitude-based variation

## Success criteria
- Caves are coherent and interesting
- Terrain has clear large-scale and local variation
- Biomes affect both surface appearance and terrain shape
- No obvious chunk seams in cave systems

## Risks
- Cave noise destroys terrain too aggressively
- Terrain generation becomes too expensive
- Too many ad hoc rules make generation hard to tune

---

# Sprint 5 — Palette Compression and Data-Oriented Chunk Storage

## Goals
- Replace dense chunk storage with production-ready local palette storage
- Add compact index packing
- Add derived occupancy and opacity masks
- Prepare for high-performance meshing

## Deliverables
- Palette-based subchunk storage
- Packed palette indices
- Occupancy mask
- Opaque mask
- Updated read/write/edit APIs

## Tasks

### Palette storage
- Implement local palette of block IDs per chunk
- Add variable bit-width packed indices
- Handle palette growth/rebuild

### Masks
- Build/maintain:
  - occupancy mask
  - opaque mask
- Update masks on edits and generation

### Migration
- Port generation and rendering to new storage
- Preserve correctness under edits

### Benchmarking
- Measure memory usage before/after
- Measure chunk generation and access overhead

## Success criteria
- New storage is functionally correct
- Memory usage drops significantly
- Meshing input data is easier to scan efficiently
- Performance is at least neutral, preferably improved

## Risks
- Packed storage complicates block edits
- Bugs in repacking logic
- Too many abstractions around storage slow down hot paths

---

# Sprint 6 — Binary Greedy Meshing

## Goals
- Replace naive visible-face meshing with binary greedy meshing
- Dramatically reduce quad count
- Improve CPU meshing throughput
- Build production meshing architecture

## Deliverables
- Face visibility mask generation
- Greedy rectangle merge per face direction
- Compact mesh output
- Neighbor-aware border meshing
- Meshing benchmarks

## Tasks

### Face visibility
- For each face direction:
  - compute visible face masks using occupancy/opaque neighbors
- Include neighbor chunk borders correctly

### Greedy merge
- Merge exposed coplanar faces by:
  - material
  - face direction
- Initial version can ignore AO/light constraints until later

### Mesh output
- Emit packed vertex data or quad descriptors
- Build upload-ready mesh structure

### Meshing system architecture
- Chunk snapshot or immutable read path for meshing jobs
- Add meshing performance counters
- Add quad/triangle count counters

### Benchmarking
- Compare:
  - naive face culling
  - binary greedy meshing
- Test on:
  - flat terrain
  - cave terrain
  - mixed biome terrain

## Success criteria
- Triangle/quad counts are much lower than naive meshing
- Meshing is stable and seam-free
- Chunk meshing throughput is high enough for streaming
- Debug counters prove real gains

## Risks
- Border handling bugs
- Merge constraints too loose causing visual artifacts
- Time lost making meshing too clever before correctness is solid

---

# Sprint 7 — Lighting: Skylight, Block Light, Ambient Occlusion

## Goals
- Add Minecraft-style lighting
- Introduce skylight propagation
- Introduce emissive block lighting
- Add ambient occlusion to mesh generation

## Deliverables
- Light storage
- Skylight propagation
- Block light propagation
- Incremental light updates
- AO in meshing/rendering

## Tasks

### Light storage
- Add sky light array
- Add block light array
- Use packed or byte storage

### Skylight
- Top-down initialization for world-generated chunks
- BFS propagation through transparent blocks
- Cross-chunk propagation support

### Block light
- Emissive blocks as sources
- Queue-based BFS update
- Support both add and remove propagation logic

### Ambient occlusion
- Compute per-face-vertex AO from neighboring occupancy
- Include AO in vertex format or packed face data
- Update merge constraints:
  - AO must match to merge

### Chunk updates
- On block edit:
  - mark chunk dirty
  - trigger light updates
  - trigger neighbor updates if border affected

## Success criteria
- Outdoor terrain shows correct skylight behavior
- Emissive blocks illuminate surrounding area
- AO improves visual quality noticeably
- Block edits update lighting without full world rebuilds

## Risks
- Light propagation complexity across chunk borders
- Removal logic bugs causing stale light
- AO prevents too many merges if integrated incorrectly

---

# Sprint 8 — Structure Placement and World Features

## Goals
- Add deterministic structure generation
- Make the world feel inhabited and varied
- Keep structure placement generation-order independent

## Deliverables
- Deterministic structure placement framework
- Tree generation
- Basic landmark or ruin structures
- Placement validation rules

## Tasks

### Placement framework
- Region/chunk-based hashed placement seeds
- Feature registry by biome/conditions
- Validation against terrain and biome data

### Structures
- Add:
  - trees
  - boulders
  - simple ruins or huts
- Structure templates stored in data format or code

### Integration
- Structure generation runs after terrain/base surface generation
- Ensure placement does not depend on generation order
- Handle cross-chunk placement safely

## Success criteria
- Structures appear deterministically for a given seed
- Trees and features do not create seams or missing parts
- World feels more organic and varied

## Risks
- Cross-chunk structure placement bugs
- Structures placed before neighboring chunks exist
- Generation order affecting results

---

# Sprint 9 — Persistence and Region File Format

## Goals
- Save/load generated and edited world data
- Add region-based disk storage
- Keep streaming and persistence asynchronous

## Deliverables
- Region file format
- Chunk serialization
- Background save queue
- Load existing chunks from disk
- Fall back to generation if absent

## Tasks

### Region format
- Design file layout:
  - header
  - chunk presence table
  - chunk offsets
  - compressed payloads
- Add version field

### Serialization
- Save:
  - palette
  - packed indices
  - light data
  - metadata
- Mark dirty chunks for save

### IO pipeline
- Background load requests
- Background writes
- Main-thread or world-thread state integration

### Correctness
- Saving and reloading should preserve edits
- Deterministic generation remains fallback only

## Success criteria
- World edits survive restart
- Chunk loading from disk is stable and fast enough
- No corrupted state from partial updates
- Save pipeline does not stall gameplay

## Risks
- Data format churn too early
- Race conditions between save and edits
- Corruption handling not planned

---

# Sprint 10 — Rendering Optimization and GPU Upload Pipeline

## Goals
- Reduce rendering overhead
- Improve mesh upload efficiency
- Prepare rendering path for larger view distances

## Deliverables
- Improved mesh buffer management
- Better batching/material grouping
- Reduced CPU-side render overhead
- Stable render debug metrics

## Tasks

### GPU upload path
- Add staging strategy
- Reuse buffers where possible
- Avoid per-frame small allocations

### Render organization
- Group visible chunk meshes by pipeline/material
- Reduce state changes
- Track draw-call count and mesh count

### Mesh format tuning
- Benchmark:
  - expanded vertices
  - compact packed vertices
- Decide whether shader-side expansion is worth future exploration

### Debugging
- Add counters:
  - draw calls
  - chunks rendered
  - triangles
  - upload bytes/frame

## Success criteria
- Stable frame times at moderate view distances
- Mesh upload path handles chunk churn without spikes
- Draw-call count is controlled and measurable

## Risks
- Premature optimization without profiling evidence
- Upload path abstractions becoming too complex
- Spending time on micro-optimizations before larger wins

---

# Sprint 11 — Block Editing, Dirty Propagation, and Update Robustness

## Goals
- Support block placement/removal cleanly
- Ensure edits trigger correct meshing and lighting updates
- Make the engine robust under frequent modifications

## Deliverables
- Public block edit API
- Dirty chunk marking
- Neighbor border invalidation
- Light and mesh update propagation
- Stable edit behavior during streaming

## Tasks

### Editing
- Implement block set/remove operations
- Update palette/masks/light source info
- Bump chunk version

### Dirty propagation
- Mark current chunk dirty
- Mark adjacent chunk dirty if edit touched border
- Queue remesh and relevant light updates

### Robustness
- Test repeated edits while moving
- Test edits near unloading/loading chunk boundaries
- Ensure stale tasks cannot overwrite edited state

## Success criteria
- Placing/removing blocks updates world correctly
- Border edits update neighboring meshes correctly
- Lighting updates remain stable under edits
- No data corruption under chunk churn

## Risks
- Hard-to-debug stale version issues
- Neighbor remesh storms
- Lighting propagation too expensive for frequent edits

---

# Sprint 12 — Profiling, Optimization, and Chunk Size Benchmarking

## Goals
- Profile the whole engine
- Make evidence-based optimization decisions
- Benchmark 16³ vs 32³ subchunks
- Establish performance baselines

## Deliverables
- Performance benchmark scenes
- Profiling reports
- Chunk size comparison report
- Optimization backlog based on measured bottlenecks

## Tasks

### Profiling
- Measure:
  - generation time
  - meshing time
  - render submit time
  - upload cost
  - lighting update cost
  - memory usage
- Use multiple scenes:
  - open plains
  - mountainous terrain
  - cave-heavy terrain
  - edited world

### Chunk size benchmark
- Build/test both:
  - 16³
  - 32³
- Compare:
  - chunk count in view
  - meshing cost
  - update latency
  - draw-call count
  - memory usage
  - streaming behavior

### Optimization pass
- Remove obvious hot-path allocations
- Tighten storage access
- Tune queue priorities
- Reduce lock contention

## Success criteria
- Team has hard numbers for architecture decisions
- Final chunk size is chosen based on real data
- Top bottlenecks are known
- Next optimization work is prioritized rationally

## Risks
- Benchmark setup not representative
- Misreading profiling data
- Optimizing the wrong subsystem

---

# Sprint 13 — Advanced Streaming and Prioritization

## Goals
- Improve streaming quality under fast movement
- Reduce visible chunk pop-in
- Prioritize the right jobs at the right time

## Deliverables
- Better chunk request prioritization
- Motion-biased prefetching
- Queue tuning
- Reduced visible stalls/pop-in

## Tasks

### Priority system
- Refine scoring with:
  - distance
  - camera forward direction
  - frustum relevance
  - neighbor completion importance

### Prefetch
- Bias requests toward player movement direction
- Tune radii:
  - simulation
  - meshing
  - rendering
  - prefetch

### Stability
- Prevent starvation of nearby chunks
- Avoid work being wasted on chunks that will unload soon

## Success criteria
- Fast movement causes less visible pop-in
- Nearby visible chunks are consistently prioritized
- Work queues remain stable under stress

## Risks
- Priority system becomes too complex
- Distant work starves important local updates
- Debugging queue behavior becomes difficult

---

# Sprint 14 — Occlusion Culling Prototype

## Goals
- Explore whether occlusion culling provides meaningful gains
- Add a first conservative occlusion culling prototype

## Deliverables
- Occlusion culling prototype
- Metrics comparing with and without culling
- Decision on whether to keep/expand

## Tasks

### Prototype options
Choose one:
- CPU conservative chunk occlusion
- GPU depth pyramid style chunk AABB occlusion later

### Metrics
- Count culled chunk meshes
- Measure frame-time improvement
- Measure CPU/GPU overhead of culling itself

### Decision
- Keep only if gains justify complexity

## Success criteria
- Measured data exists for usefulness of occlusion culling
- Prototype is conservative and artifact-free
- Team can decide whether to continue investment

## Risks
- Complexity exceeds payoff
- Incorrect culling causes popping
- Time spent here before more impactful optimizations

---

# Sprint 15 — Far-Field Rendering and LOD Research Prototype

## Goals
- Prepare for larger view distances
- Explore future LOD architecture
- Build an experimental far-field solution without disrupting near-field correctness

## Deliverables
- LOD prototype document
- Optional far mesh generation experiment
- Evaluation of architecture changes needed

## Tasks

### Research prototype
Try one:
- distant coarsened chunk meshes
- region-level terrain mesh
- clipmap-inspired far terrain representation

### Requirements analysis
- multiple mesh representations per world area
- residency tiers
- seam handling between near and far field

### Outcome
- Produce design proposal, not necessarily production-ready system yet

## Success criteria
- Team understands practical LOD path for this engine
- Near-field architecture remains compatible with future LOD
- Prototype demonstrates feasibility or rules out bad directions

## Risks
- LOD complexity explodes
- Too early to build production LOD
- Block-world semantics make chosen LOD path awkward

---

# Cross-Sprint Engineering Rules

## Every sprint must include
- profiling hooks for new systems
- debug counters and overlays where relevant
- tests for deterministic generation if generation changes
- regression checks for chunk border seams

## Do not proceed to advanced systems until
- current stage is measurable
- correctness is stable
- stale task/versioning issues are under control

---

# Suggested Ownership Split for 2 Developers

## Developer A
More engine/runtime focused:
- render loop
- renderer
- GPU upload
- chunk manager
- streaming
- culling
- profiling/debug tools

## Developer B
More simulation/world focused:
- generation
- storage
- meshing
- lighting
- structures
- persistence

## Shared
- architecture decisions
- benchmark interpretation
- chunk state/version model
- block registry and data formats

---

# Milestone Summary

## Milestone 1: Playable prototype
End of Sprint 2
- moving through streamed voxel world
- async chunk generation and meshing
- basic rendering in place

## Milestone 2: Real world generation
End of Sprint 4
- deterministic natural terrain
- biomes
- caves

## Milestone 3: Production chunk pipeline
End of Sprint 6
- palette storage
- binary greedy meshing
- scalable world data path

## Milestone 4: Visual quality and persistence
End of Sprint 9
- lighting
- structures
- saving/loading

## Milestone 5: Optimization baseline
End of Sprint 12
- measured architecture choices
- chosen chunk size
- prioritized performance backlog

## Milestone 6: Future scalability
End of Sprint 15
- occlusion/LOD direction established
- engine ready for deeper optimization

---

# Final Notes

This sprint plan is intentionally staged to avoid the classic voxel engine trap:

- trying to build every advanced system at once
- delaying a playable prototype too long
- optimizing blindly without instrumentation
- overengineering architecture before data proves it

The sequence here is:

1. get a working world  
2. make it stream  
3. make it look natural  
4. make storage/meshing fast  
5. add lighting and persistence  
6. optimize based on evidence  
7. only then explore LOD/advanced culling
