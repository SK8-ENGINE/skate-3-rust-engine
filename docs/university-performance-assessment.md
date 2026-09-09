# University: performance assessment, 9 September 2026

This investigation preserves the current rendered image and native-derived gameplay
contract. It does not lower resolution, texture precision, sampler fidelity, density,
draw distance, shadows, animation quality or physics cadence. It introduces no LOD.
No game, Steam, recomp or automated gameplay was launched. There is **no measured
FPS gain** and no conclusion yet that University is CPU- or GPU-bound on the user's PC.

Baseline: `57742e0` (the available `origin/main` at worktree creation), Bevy 0.18.1,
wgpu 27.0.1, vendored PBR/core-pipeline patches. Static inventory used the existing
fresh installation's University v14 package, SHA-256
`b63e57a84884e639c5ce433a84834a840ae96bfdacad8f34c457d28211315f42`.
The local `main` ref was older; no checkout or remote ref was modified to change it.

## Evidence and scale

The data-only inventory decoded the existing package and the ignored renderer test
ran the actual `render_texture_ids`, `render_material_ids` and `render_groups`
functions. Neither initializes a window, Bevy application or GPU.

| Quantity | Observed |
|---|---:|
| Package bytes | 281,255,761 |
| Render vertices / triangles | 2,075,425 / 1,645,617 |
| Material records / actual material batches | 8,546 / 4,870 |
| Texture records / byte-identical unique payloads | 2,046 / 2,021 |
| Decoded texture bytes / unique bytes | 772,544,512 / 766,686,208 |
| Rail records | 4,201 |
| Batches wider than 50 m horizontally / their triangles | 2,021 / 1,226,659 |
| Wider than 100 m / their triangles | 624 / 435,401 |
| Wider than 250 m / their triangles | 54 / 35,027 |
| Wider than 500 m / their triangles | 6 / 10,191 |
| Estimated occupied 32 m material cells | 19,604 |

Extent buckets overlap. The cell estimate assigns triangle centroids to an XZ grid;
it is not a measured culling result or a proposed mesh implementation. Existing
installation validation logs separately report 1,133,649 collision triangles,
5,537 query clusters and 27,008 grind primitives. These are existing log evidence,
not new runtime profiles. Decoded texture bytes exclude lightmap half-float
expansion, sampler-role duplication, mip chains, sky/character textures and GPU
allocation overhead. They must not be called VRAM consumption.

## Rendering and map data path

`config.rs::from_env` reads the selected package and computes a map fingerprint.
`skate_world.rs::validate_runtime` validates supported extensions/collision.
`world.rs::setup` takes the decoded map out of `Config`; startup does not retain
the full package permanently. `skate_world.rs::spawn` canonicalizes resources,
reindexes material groups and emits static identity-transform mesh entities.
Textures and meshes use `RenderAssetUsages::RENDER_WORLD`, allowing main-world
payloads to leave after upload. No per-frame map reparse or map-wide transform
rewrite was found in this path.

`map_render.rs::PreparedScene` reserves handles, builds detached scene commands
and publishes at a schedule boundary. `map_transition.rs::start` reads/validates
on a loader thread and constructs rendering alongside physics on another worker.
`commit` retires map-owned entities/materials/images/meshes before publishing the
new world. The decoded transition package is dropped on the worker. Live old and
prepared new worlds coexist during preparation; a temporary memory peak is expected.
GPU extraction/removal is asynchronous, so counting just main-world assets is
insufficient to establish either a leak or complete retirement.

**Culling and batching — highest-priority structural experiment, conditional on
the trace.** Material grouping reduces 8,546 source groups to 4,870 draws' worth of
mesh/material entities before Bevy visibility and batching. It combines distant
geometry with identical material identity into one larger bound. Existing frustum
and optional hierarchical occlusion operate at mesh/instance granularity: they
cannot reject individual disconnected regions inside one visible material mesh.
`graphics_menu.rs::apply` already configures `DepthPrepass` and `OcclusionCulling`;
adding those again is not an optimisation. `vendor/bevy_pbr/src/render/gpu_preprocess.rs`
and the core 3D phase nodes own preprocessing and draw execution.

If GPU invocation counts are high while most geometry is behind buildings, evaluate
spatial subdivision **only for large opaque/masked batches**, with conservative
full-triangle bounds. Keep vertex bit patterns, indices, UVs, normals, tangent
frames, materials and shadow participation unchanged. Test camera cuts and fast
turns; do not accept occlusion false negatives or pop-in. A blanket grid raises the
group estimate about fourfold, potentially worsening queue/binning/submission cost.
Transparent batches cannot be split casually: changing mesh centers changes sort
order, and coplanar/decal order can affect pixels. No split is enabled here.

**Material changes, pipeline traffic and instancing — potentially substantial,
but not established by batch count.** `retail_render.rs::RetailWorldMaterial` has
eight texture/sampler roles, per-material uniforms and a shared storage binding;
it does not declare a bindless resource layout. `RetailKey` adds two-sidedness to
Bevy's pipeline keys; alpha/MSAA/prepass and mesh layout also affect specialization.
`vendor/bevy_pbr/src/material.rs` queues and bins using pipeline/material/mesh keys.
Distinct handles are not automatically one draw merely because they use one shader.
Prepared mesh and waiting-pipeline counters plus system spans now expose the cost.

Investigate bindless/texture-array material access only if queue or binding costs
dominate and hardware limits permit it. Preserve every sampling mode, mip policy,
color-space interpretation and fallback path. Atlas packing is not automatically
lossless: borders, repeat addressing and derivatives matter. The `.skate` path
already bakes world geometry; repeated source props are not currently retained as
one local mesh plus transforms. Recovering instancing must prove all vertex data,
per-instance lighting and material inputs match. No geometry welding, approximate
matching or loss of baked lightmap distinctions is justified.

**Textures/material duplication — existing wins, limited byte-identity headroom.**
`render_texture_ids` hashes width/height/color space/payload then confirms equality,
so hash collisions cannot incorrectly merge images. Image cache keys include the
canonical texture and role. Roles deliberately differ: lightmaps expand to linear
half floats, cube maps need six layers, some data remains mip zero, and other roles
build mips and use different address modes. Combining roles solely because bytes
match can change output. Existing exact payload dedup saves only 5,858,304 decoded
bytes in University. Material canonicalization includes retail definitions as well
as PBR fields; removing apparently unused definition fields needs a full proof that
the build consumes no distinction. Texture compression/reduction is outside this
zero-degradation task unless mathematically lossless and supported end-to-end.

**Foliage, alpha and shader cost — GPU capture required.** Retail materials preserve
two-sidedness; alpha mask/blend semantics and `retail_depth.wgsl` rejection are
significant. `retail_world.wgsl` samples diffuse/lightmap and conditional detail,
normal, macro/decal/specular resources. Samples precede some rejection to preserve
implicit derivatives. The family and texture flags are material-uniform, so these
branches are not necessarily divergent across pixels. Moving a sample after
discard, flattening detail or disabling backfaces can change pixels. Specializing
families could reduce instructions at the price of pipeline variants; measure
`elapsed_gpu`/fragment invocations and inspect generated draws first. Density,
alpha cutoff, texture precision and draw-distance reductions are disallowed.

**Uploads, lifetimes and exposure — narrow safe change made.** The shared shadow,
clock and ocean PCA state in `retail_shadow.rs::upload` already updates an existing
GPU buffer without invalidating every material. This change replaces its temporary
144-byte heap vector with a fixed stack array; scalar order, little-endian bytes,
buffer, write size and scheduling are unchanged. It saves one allocation per upload
call; no frame-time benefit is claimed. The clock/PCA changes each frame, so simply
skipping updates is unsafe. `retail_exposure.rs::upload` writes 32 settings bytes;
`ExposureNode::run` creates two bind groups per view/frame and executes the meter
and tone passes. Caching bind groups keyed by the actual source/target identity is
a candidate if CPU encoding cost matters, but must invalidate on resize/MSAA/target
changes and account for ping-pong views. Added pass diagnostics measure these custom
passes without changing shader math or their order. Exposure's temporal adaptation
must not be decimated.

`render_capacity.rs` already reserves instance capacity with bounded growth before
Bevy writes buffers. Do not replace that with exact-size per-frame reallocations.
Render preparation stalls can also be presentation backpressure; the official
[Bevy profiling guide](https://github.com/bevyengine/bevy/blob/v0.18.1/docs/profiling.md)
specifically discusses this ambiguity. CPU command time is not GPU execution time.

## Simulation, transforms and scheduling

`app.rs` orders fixed input → controls → physics and Update assets → physics
presentation → animation → verification. `physics.rs::advance` uses the native-derived
clock period, runs the monolithic fixed frame and republishes cadence. `physics/frame.rs`
preserves preceding-query consumption, animation/input/state selection, movement,
shared solve and output publication order. Added spans isolate animation graphs,
collision/solve and finishing work. They do not parallelize or reorder any phase.

`skate_world.rs` constructs `BoardWorld` from the embedded retail collision archive.
`skate-core/src/physics/board_world/query_index.rs` already builds a bounds hierarchy
over authored clusters, traverses intersecting nodes and sorts matches back into
source order. Narrow phase checks volumes/triangles; wheels, feet, camera and board/
skeleton assemblies have distinct query consumers. `physics/solve.rs` also handles
dynamic/network bodies. Retail map size alone does not establish fixed-step cost.
The query index allocates result/stack vectors; scratch reuse is a plausible CPU
optimisation if profiles show query pressure. Preserve result order, tie handling,
contact retention and delayed query visibility. Replacing the solver, simplifying
collision, reducing ticks, or changing contact ordering would violate the task.

`grind_world` retains 4,201 authored splines with 27,008 primitives and an octree
provider; it is not a naive all-rails scan for every normal query. Candidate counts
and selection work need focused subspans if the fixed-frame profile implicates it.
`retail_irradiance.rs::sample` already has a six-entry, 2.5-metre hysteresis cache,
then bounds-filtered nearest-probe selection. A spatial index could help cache misses,
but must retain nearest/tie selection and hysteresis exactly.

`presentation.rs::capture` only captures a new pose generation, retains previous/
current snapshots and records replay. `animation.rs` interpolates bone transforms
for display. Snapshot copying and hierarchy propagation are candidates if those
systems dominate, not grounds to reduce bone update rate or interpolation quality.
Static map transforms are separate from these moving character hierarchies.
Multiplayer/mod/vehicle systems are additional workload; profiles should state
whether mods or online play were active rather than attributing all time to University.

`graphics_menu.rs::pace` sleeps in `Last` when capped; window presentation may add
another wait. The new frame-interval counter and CPU spans preserve both behaviors.
Do not remove waits, alter fixed cadence or infer GPU saturation from a capped FPS
number. Timing variation must be investigated without modifying native input/physics.

## Next decision requires a user capture

Use [the capture workflow](performance-tracing.md). Record 30 seconds after loading:
stand briefly, rotate the camera, then skate through a representative University
area. Repeat the same settings/route with `--trace-gpu`; record a separate startup
trace if the problem is loading or first-use hitching. Include where the slowdown
occurred, approximate time in the capture, and whether mods/multiplayer were active.
Do not change quality settings between captures.

Analyse steady-state frame-interval p50/p95/p99, fixed iteration counts, dominant
CPU slices, waits, prepared-resource/pipeline counts, and asynchronously received
GPU pass durations/invocations. A high CPU queue cost favors batching/material work;
high GPU opaque/foliage work favors conservative spatial subdivision or exact shader
specialization; dominant fixed solves favor order-preserving scratch/query work.
First-use-only stalls favor upload/pipeline cache work. Any nonzero dropped-event
count limits conclusions. Broad redesign waits for this evidence and manual visual
verification. No percentage gain or final bottleneck is asserted in this assessment.

## Verification

The Windows static-CRT release compiled successfully using the existing shared
Cargo target cache and an explicit worktree-private EXE/PDB output. The final
executable's embedded source revision was checked against `c709b5d`; PE import
inspection found only Windows system libraries, with no Bevy/Rust DLL dependency.
Four release unit tests passed: option bounds/dependencies, disabled span filtering,
nonblocking full-queue drops, and F9-style activation/stop/valid JSON export with
secret/path/log canaries omitted. The data-only exact-runtime-batch inventory test
also passed. Existing compiler warnings remain; no game or GPU device was started.
Runtime frame timings, capture overhead and visual validation remain user tests.

## First user capture: limited CPU evidence

The user recorded a CPU-only capture with build `c709b5d`. Capture SHA-256:
`6ab11b1e174049507be2506f17d35ef6cd111fea63180ceb82a147c2067cf8d3`.
It contains 1,194,743 events and 668 frame-interval samples covering approximately
3.05 seconds of active recording. The 128 MiB byte bound terminated it early;
the queue reported zero drops. The earlier timestamp range includes the armed
waiting period and must not be mistaken for recorded gameplay duration.

| Measurement | Median | 95th percentile | Interpretation |
|---|---:|---:|---|
| Main-frame interval | 3.59 ms | 7.50 ms | Instrumented, short sample; not scanout or baseline FPS |
| CPU command-buffer generation | 0.83 ms | 4.25 ms | Includes waits/descheduling; not GPU execution |
| CPU main opaque pass | 0.53 ms | 4.18 ms | Nested in command generation; do not add to it |
| Fixed collision/solve | 0.36 ms | 0.45 ms | 183 recorded fixed-step subspans |
| Fixed animation graphs | 0.12 ms | 0.22 ms | Fixed-step subspan, not all animation/presentation |

Frame-interval p99 was 8.20 ms, maximum 9.21 ms and mean 4.57 ms. These values cannot
describe a full skating route or quantify recorder overhead. Prepared mesh/image
counts stayed at 4,927/2,173 over 11 sampled observations; waiting pipelines stayed
at zero. This window supplies no evidence of asset churn or first-use pipeline
compilation. The actual window/camera target was 2560×1369 while requested settings
were 2560×1440, 100% scale, 8× MSAA, uncapped, occlusion off. No setting was changed
to obtain those measurements. Device timestamp support was present, but GPU
diagnostics were not requested, so GPU costs remain unmeasured.

The recording exposed a release instrumentation defect: system names appeared as
`<Enable the debug feature to see the name>`. Consequently, the aggregate anonymous
system time cannot identify particular game or renderer systems. The recorder now
retains Bevy's diagnostic names in release builds and tests a real headless ECS
schedule to catch this regression. A 25 µs default slice threshold limits tiny
scheduler events; applying that filter offline to this old recording would reduce
its retained events from 1,194,743 to 61,607 (about 6.4 MB with the old labels).
That is a file-volume calculation, not a measured improvement in runtime overhead.

Next: repeat with the corrected build and `--trace-gpu`, using the same scene and
settings. CPU command generation is a candidate for further investigation, but
this short trace does not yet support material/culling redesign or a physics change.

Corrected build `2a74581` passed all four release recorder tests, including actual
ECS system/schedule name assertions and the duration-filter boundary. The streaming
analyser reproduced the capture's event/sample counts and limitation flags. The
new private static-CRT release EXE/PDB built successfully; embedded revision and PE
imports were verified. The original recording and executable were retained. No
gameplay or GPU capture was launched by the agent.

## Second user capture: named CPU systems and measured GPU durations

Build `2a74581`, GPU diagnostics enabled, SHA-256
`03d97c8b8cd3980123ad6ea7c6e7c36d9d41ff2c5877077851ce93086b1747de`.
The useful recording covers 24.62 seconds and 6,997 frame intervals. It ended at
the byte bound, with zero queue drops and 12,213,517 intentionally filtered short
spans. This is sufficient for the observations below despite not reaching 30 seconds.
Another small file from this session contains only metadata and no active capture.

Frame interval: mean **3.52 ms**, median **3.11 ms**, p95 **6.33 ms**, p99 **7.11 ms**,
maximum **9.71 ms**. The mean corresponds to about 284 main-app updates per second,
not verified display FPS. Settings and actual target size match the first capture.
The samples cover a different interval and instrumentation configuration, so their
lower timings are **not an A/B performance improvement**.

| Scope | Mean | p95 | Measurement |
|---|---:|---:|---|
| Render schedule | 3.27 ms | 6.07 ms | Inclusive CPU wall time |
| Main app | 2.25 ms | 2.98 ms | Inclusive CPU wall time; overlaps render work |
| Command-buffer generation tasks | 1.27 ms | 3.59 ms | CPU work/waits |
| Directional-light mesh visibility | 0.448 ms | 0.974 ms | CPU system |
| Controller polling | 0.342 ms | 0.443 ms | CPU/OS calls |
| Character-material preparation | 0.124 ms | 0.160 ms | CPU system |
| Physics advance | 0.644 ms | 0.880 ms | Per fixed tick: 1,477 ticks, not per display frame |
| Main opaque pass | 0.191 ms | 0.259 ms | GPU timestamp duration |
| Retail exposure meter | 0.00781 ms | 0.00794 ms | GPU timestamp duration |
| Retail exposed tone | 0.0157 ms | 0.0159 ms | GPU timestamp duration |

CPU rendering/visibility is a stronger optimisation lead than main-pass shading
for this scene. GPU figures cover instrumented passes, not the entire GPU frame;
some anonymized shadow/other pass labels cannot be attributed individually from
this export. CPU scope durations include scheduler and profiler overhead. The
recorder's `end_frame` system itself averaged 0.115 ms; this is only one component
of capture overhead, not its total. No GPU saturation claim follows from these data.

Across 117 resource snapshots there were consistently 4,936 prepared meshes,
2,195 prepared images, and zero waiting pipelines. There is still no indication
of resource churn or shader compilation driving this interval. These counts
differ from the first capture, so the two recordings are not identical workloads.

The visibility source checks two distinct shadow sources: world/skater shadows
for character lighting, and layer-28 player-only shadows received by the baked
world. Removing casters or reducing cascades would change the image. It remains a
substantial lead, but no visibility shortcut is applied without an exact equivalent.
Controller polling calls XInput state and capabilities at the existing cadence;
throttling absent slots or caching subtype data could alter reconnect/input semantics,
so it is not changed under the timing constraint.

One directly supported change is implemented in `retail_character.rs::update`.
It previously used `Assets::iter_mut` to assign SH lighting to every character
material every frame. Bevy emits `AssetEvent::Modified` for those mutable accesses,
including identical assignments, causing needless preparation/binding work after
lighting has settled. The new path reads each material first and only obtains a
mutable reference if any SH float's **bits** differ. It still computes probe choice,
smoothing and shadow state every frame and updates every changing/new material in
the same frame. It uses no tolerance, cadence reduction or shader change. Signed
zeros and NaN payloads are compared explicitly. Reusable scratch storage avoids a
new per-frame allocation. This is expected to help settled-lighting frames; it may
save nothing while every material's lighting changes. No FPS gain is claimed before
a matching post-change capture and user visual test.

Validation of `d3f964b`: both character-lighting release tests passed, covering
unchanged asset events, changed values, newly bound pieces, float bit preservation
and existing shadow-source separation. The private static-CRT EXE/PDB built and
its embedded revision/system-only PE imports were verified. The local comparison
launcher records 20 seconds with unchanged settings; the preceding executable and
capture remain available. No post-change gameplay result is claimed.
