# 1. Project Goals

## Core goals
- Minecraft-like block voxel world
- Infinite procedural world generation in all directions
- Deterministic generation from a world seed
- CPU-based meshing
- GPU-based rendering
- High performance as a primary design constraint
- Clean, modular architecture
- Data-oriented design
- Multithreaded chunk pipeline
- Extensible toward:
  - LOD
  - better lighting
  - occlusion culling
  - improved terrain generation
  - networking / persistence later

## Non-goals for initial versions
- Smooth voxel terrain
- Sparse voxel octrees
- GPU meshing
- Real-time GI
- Complex ECS-driven everything
- Full-blown game logic architecture before world/rendering performance is proven

---

# 2. High-Level Technical Direction

This engine will use:

- **Chunked block world**
- **Subchunk-based storage and meshing**
- **Palette-compressed block storage**
- **Derived occupancy/transparency masks**
- **Binary greedy meshing on CPU**
- **Mesh upload to GPU**
- **Chunk/subchunk frustum culling**
- **Async generation / meshing / streaming pipelines**
- **Flood-fill block lighting and skylight**
- **Deterministic procedural generation pipeline**
- **Data-oriented hot-path structures**
- **Job-based multithreaded execution**

This is the recommended practical high-performance architecture for an editable Minecraft-like engine.

---

# 3. Recommended Rust Tech Stack

## Language
- **Rust stable**

## Windowing / input
- **winit**

## GPU API
Two viable options:

### Option A: `wgpu`
**Recommended if you want faster development and portability**
- Pros:
  - safer and easier
  - cross-platform
  - good ecosystem
  - easier to iterate
- Cons:
  - less low-level control
  - some advanced GPU-driven optimizations may be harder

### Option B: `ash` (Vulkan)
**Recommended if maximum rendering control is the top priority**
- Pros:
  - full Vulkan access
  - best long-term control for high-end rendering
- Cons:
  - much more complexity
  - slower development

## Recommendation
- Start with **wgpu**
- Keep renderer abstraction clean enough that a future Vulkan backend is possible
- Only choose `ash` from day 1 if both of you are already comfortable with Vulkan

---

## Math
- **glam**
  - fast
  - widely used
  - SIMD-friendly enough for your needs

## Serialization / config
- **serde**
- **toml** or **ron**

## Parallelism
- **rayon** for initial parallel chunk jobs
- Later possibly migrate hot scheduling to a custom task system if profiling justifies it

## Noise / procedural generation
- Start with:
  - **noise** crate or custom noise implementations
- Long-term:
  - likely custom deterministic generation module for control and performance

## Bitsets / compact storage
- standard integer bit operations first
- possibly:
  - **bitvec** only if needed for convenience
- But for hot paths, prefer custom `[u64; N]`-style bitmask arrays

## Profiling
- **tracy** via Rust integration
- plus:
  - frame timing counters
  - custom engine instrumentation
  - flamegraphs when needed

## Logging
- **tracing**
- **tracing-subscriber**

## Error handling
- **thiserror**
- **anyhow** for tools/prototypes, less so in hot runtime code

## ECS
- **No ECS for chunk/meshing/rendering core**
- If gameplay later needs ECS:
  - consider **bevy_ecs** or **hecs**
- But world storage/meshing/rendering should remain custom and data-oriented

---

# 4. World Representation

## 4.1 Coordinate system
Use world-space integer voxel coordinates.

### Types
```rust
type BlockId = u16;
type Light = u8;
type ChunkCoord = IVec3;
type LocalCoord = UVec3;
```

### World partitioning
World is partitioned into:

- **Regions** for disk storage
- **Chunks/Subchunks** for runtime storage, generation, meshing, rendering

---

## 4.2 Chunk size decision

### Recommended approach
Use **vertical subchunks** and benchmark:

- **16×16×16**
- **32×32×32**

### Recommendation
Start with **32×32×32 subchunks**, but keep chunk size a compile-time or configuration constant during early development.

### Why 32³ is attractive
- fewer chunk objects
- fewer draw calls
- better meshing amortization
- better generation overhead amortization

### Why 16³ is attractive
- finer update granularity
- less remesh work per edit
- less overdraw in chunk-level culling
- simpler memory footprints

### Final recommendation
Prototype both early with the same meshing pipeline and benchmark:
- generation throughput
- remeshing cost
- visible mesh count
- draw-call cost
- edit latency

If forced to choose in planning:
- **Use 32³ as default**
- design architecture so switching to 16³ is possible

---

## 4.3 Block data model

Each block should not be a giant struct.

Instead separate:
- block type id
- block properties from a static registry
- optional metadata if needed later

### Runtime block storage
Per subchunk:
- local palette of block IDs
- packed indices into palette
- derived occupancy masks
- derived transparency masks
- optional light arrays

### Block registry
Static block definition table:
```rust
struct BlockDef {
    opaque: bool,
    solid: bool,
    transparent_kind: TransparentKind,
    emits_light: u8,
    texture_set: TextureSet,
    merge_group: u16,
}
```

The block registry is read-only and globally shared.

---

## 4.4 Subchunk storage layout

Recommended per-subchunk representation:

```rust
struct Subchunk {
    coord: ChunkCoord,
    version: u32,

    palette: SmallVec<[BlockId; 16]>,
    indices: PackedBlockIndices, // bit-packed palette indices

    occupancy_mask: OccupancyMask,
    opaque_mask: OccupancyMask,

    block_light: Option<Box<[u8]>>,
    sky_light: Option<Box<[u8]>>,

    dirty_flags: DirtyFlags,
    mesh_state: MeshState,
}
```

### Notes
- `occupancy_mask`: block exists / solid enough to participate
- `opaque_mask`: blocks that occlude faces/light
- maintain derived masks eagerly on edits or lazily before meshing
- prefer flat arrays and compact packed storage

---

# 5. Data-Oriented Design Principles

## Rules
- Hot data in flat contiguous arrays
- Avoid pointer-heavy object graphs
- Avoid per-block heap allocations
- Avoid virtual dispatch in hot loops
- Precompute block property tables
- Use SoA or split representations where appropriate
- Maintain specialized derived masks for hot algorithms

## Hot-path data should optimize for:
- meshing
- lighting
- neighbor face checks
- chunk visibility tests
- generation fill
- serialization bandwidth

---

# 6. Core Engine Modules

Recommended crate/module layout:

```text
engine/
  app/
  core/
    math/
    types/
    timing/
  world/
    coords/
    block/
    registry/
    subchunk/
    storage/
    access/
    edits/
  generation/
    noise/
    biome/
    terrain/
    caves/
    structures/
    pipeline/
  meshing/
    masks/
    face_visibility/
    greedy/
    mesh_format/
  lighting/
    skylight/
    blocklight/
    propagation/
  streaming/
    loader/
    saver/
    region_format/
    residency/
    priorities/
  render/
    renderer/
    camera/
    culling/
    gpu_upload/
    materials/
    shaders/
  jobs/
    scheduler/
    queues/
  debug/
    overlays/
    counters/
    tracing/
```

---

# 7. World Pipeline Architecture

Each subchunk moves through stages:

```text
Requested
  -> Loaded or Generated
  -> Populated (terrain/caves/biome/structures)
  -> Light-initialized
  -> Meshed
  -> Uploaded
  -> Visible/Rendered
  -> Unloaded/Saved
```

## Important principle
Each stage should be:
- asynchronous
- independently measurable
- cancellable if no longer needed
- versioned to avoid stale results overwriting newer state

---

# 8. Chunk Manager / World Manager

## Responsibilities
- Track loaded and requested subchunks
- Maintain residency around player/camera
- Schedule generation/load
- Schedule remeshing
- Track dirty neighbors
- Handle unload/save
- Provide read access snapshots for meshing/rendering

## Recommended design
Use a central `WorldManager` with:
- chunk map
- state machine per subchunk
- dirty queues
- generation/meshing/upload priority queues

### Chunk map
Use a hash map keyed by `ChunkCoord`.

Recommended:
- standard `HashMap` first
- if needed later: faster hashers like `rustc_hash::FxHashMap` or `hashbrown::HashMap`

---

# 9. Chunk State Machine

```rust
enum ChunkState {
    Unloaded,
    Requested,
    Loading,
    GeneratingTerrain,
    GeneratingStructures,
    LightingPending,
    MeshingPending,
    Meshing,
    UploadPending,
    Resident,
    Unloading,
}
```

Each subchunk should also track:
- `data_version`
- `mesh_version`
- `lighting_version`

Meshing/upload results are only accepted if versions still match.

---

# 10. Streaming and Residency

## 10.1 Residency model
Maintain concentric distance bands around the player:

- **Simulation radius**
- **Meshing radius**
- **Render radius**
- **Prefetch radius**

Example:
- simulation: 8 chunks
- mesh: 12 chunks
- render: 16 chunks
- prefetch: 20 chunks

Tune experimentally.

---

## 10.2 Prioritization
Chunk priority should be based on:
- distance to camera/player
- visibility likelihood
- whether it blocks visible holes
- whether neighbors are needed for meshing
- player motion direction

### Priority score inputs
- distance squared
- in-frustum bonus
- camera velocity direction bias
- neighboring missing chunks penalty/bonus

---

## 10.3 Async loading/saving
Disk and generation should happen off-thread.

Region system:
- region files containing many subchunks
- compressed payloads
- background IO

---

# 11. Terrain Generation Pipeline

All generation must be deterministic from:
- global seed
- chunk coordinates

## 11.1 Generation stages
1. biome map generation
2. macro terrain height/density generation
3. cave carving / density subtraction
4. surface material assignment
5. structure placement
6. decoration placement
7. postprocess validation

---

## 11.2 Biome generation

### Recommended algorithm
Use layered low-frequency noise fields plus climate-style maps.

Generate fields such as:
- temperature
- humidity
- continentalness
- erosion
- weirdness / peaks-valleys

Then map combinations to biome IDs.

### Why
This gives more natural terrain than a single biome noise map.

### Implementation
- 2D low-frequency coherent noise
- domain warping for less artificial borders
- blend biome parameters across boundaries

### Output per x/z
- biome id
- biome weights for blending

---

## 11.3 Terrain shape generation

For a Minecraft-like world, use a **density function pipeline**, not just a heightmap.

### Recommended approach
For each `(x, y, z)`:
- compute base terrain density
- add biome-dependent shaping
- add ridges / hills / valleys
- subtract cave density fields
- threshold at 0 => solid/air

### Advantages
- supports overhangs
- supports caves naturally
- deterministic
- biome blending is cleaner

### Density stack
- continentalness: very low frequency
- terrain elevation bias
- erosion / ridge shape
- local detail noise
- domain warping

---

## 11.4 Surface rules
After stone/air density is generated:
- determine top exposed blocks
- place grass, dirt, sand, snow, etc.
- apply biome-specific surface depth rules

---

## 11.5 Cave generation

### Recommended algorithm
Use 3D density fields, not random walkers as the main cave system.

#### Cave components
- low-frequency cave noise
- ridged/tubular cave noise
- spaghetti cave fields
- cheese cave fields
- vertical shafts occasionally
- domain warping

### Why
Noise-based caves are:
- deterministic
- parallel-friendly
- chunk-local with neighbor consistency
- fast to evaluate

### Optional later
Add rare handcrafted tunnel features or worm tunnels for special cave types.

---

## 11.6 Structure placement

### Deterministic placement strategy
Use a hierarchical seeded placement system.

For each region/chunk:
- derive local RNG seed from world seed and region coords
- test structure spawn candidates
- validate biome/terrain conditions
- place structures from templates

### Structure categories
- trees
- boulders
- villages
- ruins
- dungeons
- large landmarks

### Key rule
Structure decisions should be independent enough that generation order does not matter.

Use deterministic hashing:
```text
seed' = hash(world_seed, structure_type, chunk_coord, region_coord)
```

---

# 12. Meshing

## 12.1 Chosen algorithm
Use **binary greedy meshing**.

This is the main meshing algorithm for the project.

### Why
- much faster than naive per-voxel face emission
- much fewer quads
- good fit for block worlds
- highly compatible with data-oriented masks
- strong CPU performance

---

## 12.2 Meshing overview

For each subchunk:
1. gather block visibility info including neighbors
2. compute visible face masks for each direction
3. split by material / light / AO compatibility
4. greedily merge visible faces into maximal rectangles
5. emit compact quads
6. upload mesh

---

## 12.3 Face visibility computation

For each direction:
- visible face if:
  - current voxel is solid/visible
  - adjacent voxel does not occlude this face

Use occupancy/opaque masks and neighbor subchunk border data.

### Important
Meshing must read 1-voxel neighbor context around chunk borders.

That means meshing depends on adjacent subchunks being available or using temporary border data.

---

## 12.4 Face merge constraints

Faces may merge only if identical in:
- block/material
- texture mapping mode
- AO pattern
- light values if baked into vertices
- transparency class
- face orientation

This prevents visual artifacts.

---

## 12.5 Mesh format

### Recommended initial format
CPU emits indexed triangles or non-indexed vertices for simplicity.

### Long-term preferred format
Compact quad descriptors:
```rust
struct Quad {
    origin: [u16; 3],
    size: [u8; 2],
    axis: u8,
    material: u16,
    ao: u8,
    light: u16,
}
```

GPU expands quads in shader or CPU converts to vertices before upload.

### Recommendation
Start with a straightforward packed vertex format:
```rust
struct Vertex {
    pos: [u16; 3],
    normal: u8,
    uv: [u16; 2],
    material: u16,
    light: u8,
    ao: u8,
}
```

Then optimize later if needed.

---

## 12.6 Incremental remeshing

Initial implementation:
- remesh whole subchunk when dirty

Planned optimization:
- dirty region tracking
- remesh throttling and coalescing
- prioritization near player
- border-triggered neighbor remesh only when required

### Important
A block edit can require:
- current subchunk remesh
- adjacent subchunk remesh if border touched
- lighting update
- maybe structure integrity updates later

---

# 13. Lighting

## 13.1 Initial lighting model
- skylight
- block light
- vertex ambient occlusion

This is the best practical baseline for a Minecraft-like engine.

---

## 13.2 Skylight

### Algorithm
Flood-fill style propagation:
- top-down initial fill from open sky
- lateral/downward propagation through transparent blocks
- decrement light by 1 per step unless special rules apply

### Notes
- store skylight separately from block light
- use nibble-packed or byte-packed values
- chunk-border propagation must be handled incrementally

---

## 13.3 Block light

### Algorithm
Queue-based BFS propagation from emissive blocks.

On block add/remove:
- perform incremental light update
- support removal and re-propagation

---

## 13.4 Ambient occlusion

Use standard voxel corner AO for visible faces.

### AO computation
For each face vertex:
- inspect 3 neighboring occupancy samples
- derive AO in [0..3]

### Important
AO values affect greedy merging compatibility.

---

# 14. Rendering

## 14.1 Rendering model
- chunk/subchunk meshes rendered by GPU
- texture atlas or array textures
- chunk-level culling
- batched material pipeline

---

## 14.2 Culling

### Initial
- frustum culling per subchunk AABB

### Later
- hierarchical occlusion culling
- maybe GPU depth pyramid occlusion

---

## 14.3 Draw submission

### Initial
- one draw per mesh/material group

### Later
- indirect drawing
- larger upload buffers
- reduced CPU driver overhead

---

## 14.4 Materials / texturing

### Recommended
Use texture arrays if possible.

Why:
- easier than atlas bleeding management
- cleaner indexing in shaders
- good for block materials

Fallback:
- atlas if API/platform constraints require it

---

## 14.5 Transparency
Initial version:
- only support simple cutout blocks (leaves, glass maybe deferred)
- sort transparent chunks separately later if needed

Recommendation:
- phase 1: opaque only + cutout
- phase 2: proper transparent pass

---

# 15. Parallelism and Multithreading

## 15.1 Job categories
- chunk load
- terrain generation
- biome generation
- structure generation
- lighting
- meshing
- serialization
- GPU upload staging preparation

---

## 15.2 Recommended execution model
Use a job system with:
- thread pool
- priority queues
- chunk-versioned task outputs
- main-thread render submission
- minimal locking

### Initial implementation
- Rayon-based worker execution
- custom priority queues managed by `WorldManager`

---

## 15.3 Concurrency design rules
- chunk data should not be freely mutable from many threads
- meshing/generation should work on immutable snapshots or exclusive chunk jobs
- use versions to reject stale results
- avoid giant world mutexes
- use message passing for completed tasks

---

# 16. Persistence

## 16.1 Region format
Store many subchunks per region file.

### Recommended region key
- region size e.g. 16×16×16 subchunks or 32×32×32 depending on file strategy

### Contents
- header
- presence table
- offsets
- compressed chunk payloads
- metadata/versioning

---

## 16.2 Save strategy
- save only modified/generated chunks
- background serialization
- delayed/coalesced writes
- crash-safe temp file or journaling later

---

# 17. LOD Future Plan

LOD is not part of the first milestone, but architecture should support it.

## 17.1 Future LOD approach
Recommended future direction:
- keep near field full-resolution chunks
- generate far-field simplified chunk meshes
- possibly clipmap-like terrain representation for long distances

### Not recommended initially
- octree world storage
- transvoxel complexity for a block world unless terrain style changes

---

## 17.2 LOD architecture requirement
Current chunk manager should be able later to store:
- multiple mesh representations per region distance band
- residency tiers
- far mesh caches

---

# 18. Debugging and Instrumentation

This is mandatory.

## Debug overlays should show:
- chunk states
- loaded/generated/meshed/uploaded counts
- active jobs by type
- queue lengths
- visible chunk count
- triangles/quads rendered
- meshing time
- generation time
- lighting time
- upload bandwidth
- memory usage
- chunk invalidation reasons

## Profiling
Integrate Tracy early.

Every major stage should be timed:
- generation
- structures
- lighting
- meshing
- upload
- render submit

---

# 19. Recommended Development Phases

## Phase 1: minimal vertical slice
Implement:
- window + camera
- chunk coordinates and world manager
- basic dense chunk storage
- deterministic flat/noise terrain
- naive face-culling meshing
- basic rendering
- frustum culling
- simple chunk streaming around player

Goal:
- playable prototype
- validate chunk pipeline

---

## Phase 2: proper storage and meshing
Implement:
- palette-compressed chunk storage
- occupancy/opaque masks
- binary greedy meshing
- neighbor-aware border meshing
- chunk versioning
- asynchronous generation/meshing jobs

Goal:
- strong meshing performance
- stable world pipeline

---

## Phase 3: terrain pipeline
Implement:
- biome maps
- density-based terrain
- caves
- surface rules
- deterministic structure placement
- save/load region files

Goal:
- natural world generation
- persistence

---

## Phase 4: lighting and polish
Implement:
- skylight
- block light
- AO
- lighting-aware meshing
- cutout blocks
- chunk update propagation

Goal:
- world looks good and supports edits

---

## Phase 5: rendering optimization
Implement:
- compact mesh format
- upload staging improvements
- indirect draw path if supported cleanly
- better texture system
- performance tuning with Tracy

Goal:
- reduce CPU render overhead
- scale view distance

---

## Phase 6: advanced optimization
Implement selectively:
- occlusion culling
- dirty region remeshing
- better streaming priorities
- far-field terrain mesh generation
- multistage LOD experiments

Goal:
- large world scalability

---

# 20. Key Algorithms Chosen

## World storage
- palette-compressed chunk storage
- bit-packed local indices
- occupancy and opaque masks

## Meshing
- binary greedy meshing

## Lighting
- BFS flood-fill skylight
- BFS flood-fill block light
- vertex ambient occlusion

## World generation
- layered deterministic noise fields
- density-based terrain
- 3D noise-based caves
- climate-style biome maps
- hashed deterministic structure placement

## Rendering
- chunk mesh rendering
- frustum culling
- future indirect drawing
- future occlusion culling

## Traversal / picking
- Amanatides & Woo voxel traversal

---

# 21. Risks and Mitigations

## Risk: overengineering too early
**Mitigation:** ship vertical slice first, then optimize with profiling.

## Risk: chunk synchronization bugs
**Mitigation:** strict chunk state machine + versioning.

## Risk: meshing border artifacts
**Mitigation:** explicit neighbor border sampling contract.

## Risk: lighting update complexity
**Mitigation:** implement world generation and meshing first; add lighting after pipeline is stable.

## Risk: too much abstraction hurting performance
**Mitigation:** keep hot systems custom and flat; avoid ECS in core world pipeline.

## Risk: choosing wrong chunk size
**Mitigation:** benchmark 16³ and 32³ in phase 2 before locking in.

---

# 22. Coding Standards / Design Rules

## Performance rules
- no heap allocations in hot inner loops
- no dynamic dispatch in meshing/generation/render hot paths
- avoid unnecessary bounds checks where proven safe
- prefer flat arrays and explicit indexing
- cache block property tables
- batch work by chunk/subchunk

## Architecture rules
- subsystems communicate via explicit data/state transitions
- chunk tasks are versioned
- renderer consumes immutable mesh data
- world generation is deterministic and order-independent
- profile before major refactors

---

# 23. Initial Concrete Decisions

## We will implement first:
- `winit`
- `wgpu`
- `glam`
- `rayon`
- `serde`
- `tracing`
- Tracy integration

## We will build:
- custom chunk manager
- custom world storage
- custom meshing pipeline
- custom generation pipeline

## We will not initially build:
- ECS-centric core
- Vulkan-only renderer
- LOD system
- occlusion culling
- networking
- advanced transparency
- GPU meshing

---

# 24. Suggested Initial Rust Crates

```toml
[dependencies]
winit = "0.29"
wgpu = "0.19"
glam = "0.27"
rayon = "1"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
tracing = "0.1"
tracing-subscriber = "0.3"
thiserror = "1"
anyhow = "1"
hashbrown = "0.14"
smallvec = "1"
bytemuck = { version = "1", features = ["derive"] }
noise = "0.9"
```

Optional:
```toml
rustc-hash = "1"
parking_lot = "0.12"
```

Profiling:
- Tracy crate integration as appropriate for current ecosystem

---

# 25. Example Runtime Flow

```text
Player moves
  -> residency update
  -> request missing subchunks
  -> load from disk or generate deterministically
  -> build palette/masks
  -> initialize light if needed
  -> queue meshing when neighbors available
  -> upload mesh to GPU
  -> frustum cull
  -> render visible meshes
  -> unload distant chunks and save dirty ones
```

---

# 26. Final Recommendation Summary

This project should be built as a **custom data-oriented chunk engine** with:

- **32³ default subchunks** (benchmark vs 16³ early)
- **palette-compressed storage**
- **occupancy and opaque bitmasks**
- **binary greedy CPU meshing**
- **deterministic density-based terrain generation**
- **3D noise caves**
- **climate/parameter biome maps**
- **hashed deterministic structure placement**
- **flood-fill skylight and block light**
- **AO baked into chunk meshes**
- **Rayon-based async chunk pipeline**
- **wgpu renderer first, Vulkan maybe later**
- **strict chunk state/versioning**
- **strong instrumentation from the beginning**

This is the best balance of:
- performance
- maintainability
- practicality
- future extensibility
