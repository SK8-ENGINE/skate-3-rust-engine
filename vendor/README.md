# Local Bevy patch

## Bindless material resource reuse

`bevy_pbr/src/material_bind_groups.rs` chooses a fitting slab requiring the fewest
new resource slots instead of first-fit allocation. It prefers already resident
textures, then fuller slabs, while keeping existing allocation/refcount/free logic
and capacity limits. This adds work during material allocation, not each frame.
The game's explicit Vulkan shader probe tests reuse and retirement using real GPU
texture identities. See `docs/cpu-followup-optimizations.md` for scope and limits.

## Conservative occlusion depth pyramid

`bevy_core_pipeline` is vendored from crates.io 0.18.1 with its original licenses.
Its `src/experimental/mip_generation/{mod.rs,downsample_depth.wgsl}` backports
[Bevy #22603](https://github.com/bevyengine/bevy/pull/22603), commit
`43f4552308a29f4130c3ad71ea65d48f77daea6e`. The upstream file paths changed
between releases; the algorithm is the upstream fix. Power-of-two pyramid
levels and conservative source sampling prevent false occlusion at arbitrary
viewport sizes, including scaled targets and MSAA. The associated meshlet
texture-dimension fix is included in `bevy_pbr/src/meshlet/meshlet_cull_shared.wgsl`.
The game uses ordinary meshes, not meshlets.

The late mesh preprocessing pass in 0.18.1 also binds its indirect dispatch
buffer as writable storage, which fails wgpu validation when occlusion is on.
`bevy_pbr/src/render/{gpu_preprocess.rs,mesh_preprocess.wgsl}` now declares that
binding read-only in the late pass. Its counters are plain `u32` in that shader
variant, preserving the same buffer layout; the early producer retains atomic
counters and writable storage. No dispatch count or culling decision changes.

Unused depth-pyramid mip slots previously shared one dummy storage texture.
When the pyramid was short enough that the ReadWrite SPD intermediate (`mips[5]`)
was also a dummy, wgpu rejected the dispatch: `STORAGE_WRITE_ONLY` and
`STORAGE_READ_WRITE` on the same texture in one usage scope. Padding now uses one
dummy texture per binding slot, dummies are 1×1 (not 0×0), zero-sized viewports
clamp safely, and pyramid side lengths are at least 32 so that intermediate mip
exists as a real subresource.

`bevy_pbr` is the unmodified crates.io 0.18.1 source except for the changes
described below. Its original MIT and Apache licenses are included. Cargo selects
it through the workspace `[patch.crates-io]` entry; the Cargo registry is untouched.

## Cached lightmap bindings

Bevy 0.18.1 rebuilds a bind group for every allocated lightmap slab and every
render phase on each frame, even when all bound GPU resources are unchanged.
On San Vanelona, mesh binding preparation measured roughly 10 ms per frame.

Local changes are restricted to:

- `src/lightmap/mod.rs`: each slab retains at most eight bindings. Lookup compares
  the actual wgpu buffer identity, binding offset and size, and layout identity.
  Texture insertion/removal clears the cache. FIFO eviction bounds retained old
  buffers when phases disappear or instance buffers resize.
- `src/render/mesh_bindings.rs`: reuse or create the binding through that cache.
  The lightmap slab parameter is now mutable.
- `src/lightmap/cache_tests.rs`: explicit GPU regression test.
- `src/render/mesh.rs`: retain complete lookup tables for both alternating
  instance buffers per phase. Keys include buffer identity, offset, size, layout,
  and the lightmap binding revision. A slab creation, texture upload, or removal
  changes that revision. Tables are shared by `Arc`, so a cache hit avoids
  rebuilding and cloning thousands of individual entries. Keep only two tables
  per phase, and discard caches for phases that disappear.
- `src/render/mesh.rs`: cache the IDs of meshes with morph targets when
  `RenderAssets<RenderMesh>` changes. Each phase still rebuilds its dynamic morph
  bindings from the current GPU resources, but skips scanning static city meshes.

Writing new instance data into an existing buffer does not require rebuilding a
bind group. Changing the buffer, bound range, layout, or texture does. The patch
does not skip extraction, uploads, animation, or visibility updates. It retains
the existing single-texture and binding-array rendering paths and shaders.

Run the Vulkan test explicitly:

```powershell
cargo test -p bevy_pbr --lib cache_tracks_binding_identity_and_texture_replacement --locked -- --ignored
```

It covers cloned versus replaced buffers, changed binding ranges/layouts, writes
to an existing buffer, texture insertion/removal, bounded eviction, and actual
bind-group construction/reuse, alternating-buffer table reuse, table revisions,
and morph-target additions/replacements/removals. Regular game tests and a city screenshot/benchmark
also exercise the patched renderer.

Upstream references reviewed September 6, 2026:

- [Bevy issue 23595](https://github.com/bevyengine/bevy/issues/23595) reports
  increasing `prepare_mesh_bind_groups` cost. That report points at mesh scanning;
  local measurements and source inspection identified lightmap binding recreation
  as the opportunity addressed here.
- [Bevy 0.19 rendering improvements](https://bevy.org/news/bevy-0-19/#render-big-scenes-faster)
  explain further GPU batching and CPU overhead reductions. An engine migration
  is separate from this small patch; 0.19.1 source still rebuilds lightmap groups.

When upgrading Bevy, reassess this patch and rerun the GPU test, stationary and
moving-camera benchmarks, and visual comparison. Do not assume an unchanged
private renderer implementation across engine versions.

## Fade alpha in MeshTag

`bevy_pbr/src/render/pbr_fragment.wgsl` (`pbr_input_from_standard_material`) and
`bevy_pbr/src/render/pbr_prepass_functions.wgsl` (`prepass_alpha_discard`) replace
the material's `base_color.a` with `bitcast<f32>(tag ^ 0x80000000)` when the
mesh's `MeshTag` is not 0. The game's NPC / ped fade (`living_world::npc_skaters::present_fade`,
`fade_tag`) writes the fade alpha there instead of into a per-NPC material copy
every frame, which made Bevy rebuild that material's bind group each frame; the
value is the same f32 the copy held, so drawing and the blended shadow's discard
are unchanged. Meshes without a `MeshTag` (tag 0) are untouched; nothing else in
the game sets one, so a mod or feature that wants `MeshTag` must use another
encoding or stop the fade from owning it. A negative value marks an opaque source
material (2026-11-08): the shaders use its absolute value as the alpha and, after the
base colour texture, set the alpha to it again instead of fade x texture alpha (opaque
retail textures keep other data in alpha; clothes and boards vanished during fades
while skin showed). The ped shader only ever gets positive values. Doc:
`docs/hails-additions/34-physics-performance.md#fades-meshtag`.

## Serial specialization check

`bevy_pbr/src/material.rs` `check_entities_needing_specialization` collects the entities whose mesh or material
changed with a plain `iter()` instead of `par_iter()` (`SKATE_SERIAL_SPECIALIZATION_CHECK=0` restores the parallel
pass; a `SPECIALIZATION_CHECK` log line names the mode). It runs once per material type every frame (about ten types
in the game); the per-entity test is a few change-tick compares, so the parallel version's compute-pool scope cost
more than the work (about 0.1 ms per type per frame even for a type with one or two entities). The same entities
are collected; the order of the list differed run to run already and does not reach the result (each entity is
specialized on its own). Doc: `docs/hails-additions/34-physics-performance.md#serial-specialization-check`.

## Transform propagation workers yield

`bevy_transform` is the unmodified crates.io 0.18.1 source except `src/systems.rs` `parallel::propagation_worker`:
an idle worker (queue lock busy, or queue empty while another worker still works) calls `std::thread::yield_now`
after `SKATE_TRANSFORM_YIELD_SPINS` failed polls (default 64; 0 = never yield, upstream's behaviour, for A/B checks)
instead of spinning on, and drops the queue lock before it yields.
Why: `propagate_parent_transforms` starts one worker per compute thread and the idle ones spun without ever yielding.
A Windows Performance Recorder trace with context switches (2026-11-02, `.local/research/b230-process-stall-candidates.md`
"WPR capture") showed the worker that still held work preempted for a moment and then kept off its CPU for 32 ms by
spinning workers at a boosted priority; the frame (main thread, and the render thread's command buffer tasks in the
same pool) waited with it: the 20 to 90 ms in-play stalls. On Windows `yield_now` is `SwitchToThread`, which runs
another ready thread of that processor. The work and its order inside a worker are unchanged, and each GlobalTransform
is computed from its parent as before, so the results are the same. Doc:
`docs/hails-additions/34-physics-performance.md#transform-propagation-stalls`. On a Bevy upgrade: check whether
upstream's worker still spins without yielding; re-apply or drop the patch.

## SDL3 static C runtime

`sdl3-sys` is vendored from crates.io 0.7.1+SDL-3.4.16 with its original license.
The only change is in `build.rs`: when building SDL from source for an MSVC target,
it defines `CMAKE_MSVC_RUNTIME_LIBRARY` to match the C runtime Rust links
(`MultiThreaded` with `+crt-static`, as release packages and CI use; otherwise
`MultiThreadedDLL`). SDL's CMake project enables policy CMP0091, which ignores
the `-MT` flag cmake-rs adds, so without this the static-CRT link fails with
unresolved `__imp_*` CRT symbols. Development builds (`scripts/Build.ps1`) are
unaffected. When upgrading `sdl3`/`sdl3-sys`, re-vendor and reapply this change,
or drop it if upstream cmake-rs/sdl3-sys handle the runtime themselves.
