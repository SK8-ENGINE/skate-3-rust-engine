# Physics performance: hardware FMA and parallel NPC skater steps (2026-10-15)

## Problem

With NPC Skater Physics on, the game hitched: about 110 frames a minute took 9 to 13 ms against a 3.7 ms median (user,
2026-10-15: "there are lots of hitches and fps drops during the session"). Every frame that runs a 60 Hz physics tick
cost about 5 ms more than the frames between ticks. Retail ran a player and three AI skaters at full physics on an Xbox
360, so the cost pointed at waste in our code, not at the amount of work.

## Root cause

A CPU profile (Windows Performance Recorder, muted 60 s run at the PCU Library, top functions kept in
`.local/research/profile-2026-10-15/top-functions.txt`) had `fmaf` at the top of our own code: the C runtime's
**software** fused multiply-add. The physics ports use `f32::mul_add` throughout to match the console's `fmadds` bit
for bit, and the build targeted baseline x86-64 without the FMA instruction set, so every `mul_add` was a function call
into a software routine. The solver kernels that call it (`apply_impulse_changes`, `drive::solve`, `joint::solve`)
came next.

Two smaller findings along the way:
- The per-tick sync of an NPC worker (below) deep-copied the prop collision layer's template triangles, about 0.5 ms
  per NPC per tick; the instance list is shared behind an `Arc` now.
- The mod bridge's per-frame JSON observation showed in the profile with the four dev mods loaded; separate todo
  `.claude/todo/mod-json-cost.md`.

## Change

1. **Hardware FMA** (`.cargo/config.toml`, and `+fma` next to `+crt-static` in `scripts/Build-Release.ps1`,
   `scripts/build-multiplayer-test.ps1`, `tools/build_verified_windows.ps1`, whose `RUSTFLAGS` variable overrides the
   config file): `-C target-feature=+fma`. Hardware FMA and `fmaf` are both exactly rounded, so results are
   identical; Rust never fuses `a * b + c` by itself, so no other arithmetic changes. The game needs an x86-64 CPU with
   FMA3 (Intel Haswell / AMD Piledriver, 2013, or newer); `main` checks at start and shows a message instead of an
   illegal-instruction crash.
2. **Parallel NPC skater steps** (`living_world::npc_sim::run_steps`): each simulated NPC's physics step runs on its
   own worker (`GamePhysics::npc_worker`) on the compute pool. The collision world, the prop world, the dynamic props,
   the grab scene and the data are shared behind `Arc`s (`physics/world_handle.rs`: the world plus per-worker query
   scratch, `BoardWorld::query_primitives_in` in skate-core); settings and proxies are copied per tick
   (`sync_npc_worker`); workers are released after the phase so the player's step can change the shared parts.
3. **Order fixes found on the way** (needed for the parallel step, and real bugs in the serial one): the per-skater
   collision settings (wheel / truck / deck materials, truck collisions, deck child enables) were not swapped with an
   NPC's context, so an NPC's slide or bail materials leaked into the next NPC and the player's board
   (`SkaterSettings`); and each NPC solved against proxy bodies the previous NPC's solve had moved (now all against the
   post-player state). The grab scene is refreshed once per NPC phase (each NPC rebuilt the same one), and an NPC skips
   the prop carry (it returns before touching the props with nothing held and no grab).

## Verification

- `physics::skater_context_tests::npc_skaters_stepped_in_parallel_match_the_serial_steps_bit_for_bit` (data-gated):
  two NPCs and the player, 240 ticks, every deck's position and velocity bit-equal between the serial order and the
  parallel workers. `the_player_is_bit_identical_with_a_simulated_npc_skater_in_the_same_world` still passes.
- skate-game physics + living-world suites (258 passed), skate-core board world (23 passed).
- Muted runs at the PCU Library (`.local/poc-tests/npcsim.sh`, `MAP=University TELEPORT=dist_university_library`,
  radius 150 m, four dev mods), new `FRAME_HITCH` phases `solve_queries / solve_contacts / solve_solver / solve_rest /
  solve_npc` and `NPC_SIM_STEPS`:

| | morning (serial) | parallel, software FMA | parallel + hardware FMA |
|---|---|---|---|
| hitch frames per minute | 110 | 111 | 80 |
| typical hitch frame | 9.0 ms | 9.8 ms | 6.5 ms |
| median frame | 3.7 ms | 3.8 ms | 3.1 ms |
| fixed physics step | 5.5 ms | 6.2 ms | 3.3 ms |
| solve (all skaters) | 2.9 ms | 3.7 ms | 1.1 ms |
| solver | 2.3 ms | 3.1 ms | 0.7 ms |

## Open

- Frame blending for simulated NPCs (their pose is drawn between the previous and current tick, like the player's
  presentation) is in the same change set; not yet seen by the user.
- Remaining per-frame costs outside physics (audio ~1 ms, mod JSON) and `tracing` span filtering in hot code.

## Code reference: physics/world_handle.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/world_handle.rs`; each comment there links its section here.

### World handle

The static collision world as the physics step sees it: the geometry shared behind an `Arc`, plus this step's own query scratch. The player's `GamePhysics` and every simulated NPC skater's worker hold a handle to the same world, so their steps can run on separate threads (a query only reads the world). Reads go through `Deref`; the few writes (map load, prop streaming, tests) need the world not to be shared at that moment.

## NPC and replay per-frame allocations, first pass (2026-10-28, research b620)
Behaviour-identical by construction, each change with a reference test or no arithmetic moved:
- `npc_skaters.rs` advance: the Mirror decider borrows `branches` / `tricks` instead of cloning the whole history each
  tick; `others` / `in_use` built once per NPC per tick, not per step (the loop only moves this NPC's own entry,
  which `others` leaves out); `npc_trick_profile` returns a `Cow` (borrowed unless a mod override applies; still read
  every tick, so overrides stay live).
- `puppet_layers`: `sequence_ends_in` replaces `ends_with(format!("+{b}"))` (test `sequence_ends_in_matches_the_format_reference`).
- `ReplayBuffer::record`: `encode_into` writes into a reused scratch, no `moved` / `mask` Vecs; a key is cloned only
  when it is new (test `encode_into_matches_reference`, 10 000 generated poses, byte equality with the old encoder).
- `replay/world.rs` `refresh_joints`: compares joint lists via iterators, collects only on a change.
Superseded in part by the second pass below.
## NPC per-frame allocations, second pass (2026-10-29, research b620 items 1b..1e, 2, 3d)
Proof first: `npc_present_pose_and_advance_match_golden` (headless, always runs) and the data-gated
`npc_present_pose_posed_matches_golden` (stock skater runtime, test joint bindings, the posed path including the fakie
channel tree) hash every NPC's `NpcReplay`, `NpcPuppetClip`, root and joint `Transform` bits and the `NpcSkaterEvent`
stream over 1440 frames at 144 Hz. Both digests were taken before the changes and did not move after them.
- `puppet_layers_into`: each history entry resolved once, in order, with no `resolved` Vec; `npc_puppet_layers_into`
  takes the phase history (at most `PHASE_HISTORY` = 6) into a stack array.
- `present_pose`: a `Local<NpcPoseScratch>` holds the layers, layer stance bits, fakie channel commands
  (`tree_commands_into`), pose commands / locals / globals (`puppet_pose_in_flags_into`, `stance_tail_into`) and the joint
  transforms (`pose_transforms_into`) for both tiers; the top layer is borrowed, not cloned; `NpcPuppetClip` is compared
  field by field and written in place only on a difference (same value and change ticks as building a new one).
- `advance`: a `Local<AdvanceScratch>` holds `players`, `order`, `others`, `in_use`, the cursor events and the finished
  list.
The old functions stay as wrappers over the `_into` ones. The fakie tree cache (1f) followed, see the next section.
Allocation counts and timings not measured.

## Fakie channel tree cache (2026-10-29, b620 item 1f)
Problem: while an NPC's fakie channel weight is above 0, `present_pose` built the whole `B_FAKIE_CHANNEL` tree
(`MotionAnimation::build_tree`, a `PlaybackTree` from metadata) every frame for every such NPC.

Change: `graph_host::motion::cached_tree_commands_into` keeps one built tree in `NpcPoseScratch::fakie_tree` (shared by
all NPCs) and re-runs `set_attributes`, `set_time` and `evaluate` on it. It rebuilds when the tree name (mod key
`skater_clips["fakie_channel"]`), the skater's `construction_values` or the attributes differ from the ones it was built
with, drops the tree on any error, and `present_pose` drops it when the `SkaterRuntime` resource is (re)inserted or gone.

Why the output is the same as a fresh build (reading of `skate-core/src/animation`):
- Clip leaves: `ClipClock::set_time` resets `time`, `previous_time` and `loops_since_evaluation`; this path never calls
  `set_speed` or `advance`, and `evaluate` with `update_history: false` never calls `commit_evaluation`.
- `PhaseBlend::set_attributes` skips its update when the parameter moved less than 1e-4, `SelectionSpace::select` keeps
  its first pick, and `BlendSpace` keeps its simplex. Each holds history, but with the same attributes every frame the
  kept state is what a fresh tree computes on that frame. Hence the attributes are part of the cache key: per-frame
  torso values (retail 82BAC778: manual 1.0, powerslide 0.0) rebuild the tree rather than reuse a stale pick.
- `set_time` sets every child that `evaluate` then reads (`PhaseBlend` left / right, the current simplex, the selected
  candidate), so another NPC's time left in the shared tree is overwritten.
- `PlaybackTransition` state (`elapsed`, `sequence_complete`) changes only in `advance`, never called here.

Verification: the new data-gated test `npc_fakie_tree_cache_matches_a_fresh_build` compares the cached commands with a
fresh build over 400 steps (times past the tree's length, torso 0 / 0.25 / 0.5 / 1, a construction value change, a
name that does not build); it passes, both NPC golden digests did not move, and the full skate-game suite is green
(978 passed, 2026-10-29).

## Gameplay frame-time pass: readouts, walking ground query, DMO count (2026-11-02, USER-ASKS 467)
Goal: lower frame time with NPC skaters, peds and traffic around, with gameplay unchanged.

### Readouts (observation only)
- `SYSTEM_TIMING` kept a name slot per system and stopped at 2048. The living world's FixedUpdate systems were named after the
  slots ran out, so they were never timed: in the user's 2026-11-01 trace-all session the fixed step had 1.6 ms per step of
  unattributed "gaps". Now 8192 slots and a one-time warning when they run out; gaps fell to 0.2 ms per step.
- New per-second lines under `SKATE_SYSTEM_TIMING=1`: `LIVING_TIMING` (the living world's own spans, ms per frame),
  `PHYSICS_PHASES` (the physics phase timers, which `FRAME_HITCH` only prints on hitch frames) and `MATERIAL_EVENTS` (material
  assets added / modified per second per type).
- Bench: `.local/perf-bench/lw_bench.sh` (release exe, University library, muted, player off the board, NPC Skater Physics
  on, 150 s; marks a run DISTURBED when another game process runs at the same time) and `system_timing_summary.py`.

### Where the time goes (desktop, release, 2 NPC skaters, 15 peds)
- Living world on vs off (`SKATE_LIVING_WORLD=0`): frame p50 4.00 vs 3.02 ms, p99 7.38 vs 5.69 ms; main thread +0.86 ms per
  frame, render thread +0.48 ms.
- Main thread, per frame: NPC skater physics 0.18 ms, Bevy material specialization checks about 0.25 ms in total, transform
  propagation and visibility about 0.13 ms, ped think + advance 0.09 ms; the other living world systems are each below
  0.01 ms.
- Render thread: `prepare_material_bind_groups` +0.36 ms, from material writes: 450 to 1300 `PedMaterial` and 1000 to 1500
  `StandardMaterial` modifications per second (the NPC / ped fade copies; peds fade by camera distance, so a ped in the fade
  band changes alpha whenever the camera moves). Not on the critical path today (render 1.9 ms vs main 3.7 ms).
- CPU profile (WPR, 150 s): inside the walking player's physics step, a third of the samples were
  `ground_query::world::with_world_scene` copying every static edge of the map into a new Vec on each ground query.

### Changes
1. `with_world_scene` reads the map's static edges in place (`Scene::static_metadata`) instead of copying them. `append`
   converts one edge at a time with the same field copies, in the same order, with the same 40-edge cap.
   Test `static_edges_read_in_place_match_the_converted_copy`: 600 random edges, 400 queries, bit-equal candidates from the
   old copied list and the in-place list.
2. `stream_dmos` compared `dmo_placements().len()` every fixed tick, which built and sorted a placement per map prop;
   now `dmo_placement_count()` (the same filter, counted); the placements are built only when the census refills.

### Result (interleaved A/B, 2 runs each, no run disturbed)
| | base | new |
|---|---|---|
| player physics (`physics::advance`, walking), per step | 1.28 / 1.32 ms | 0.87 / 0.85 ms |
| `stream_dmos`, per step | 0.028 / 0.029 ms | 0.007 / 0.007 ms |
| fixed step | 2.82 / 2.86 ms | 2.39 / 2.37 ms |
| frame p50 / p90 / p99 | 3.78 / 6.08 / 7.14, 3.78 / 6.10 / 6.90 ms | 3.75 / 5.68 / 6.88, 3.68 / 5.65 / 6.62 ms |

### Second batch (user 2026-11-02: spans off unless profiling; menu VM first; better ways for grab props and fades)
3. **Bevy system spans off unless something reads them** (`profiling::init`): without a Chrome capture and without
   `SKATE_SYSTEM_TIMING` / `SKATE_FIXED_TIMING`, the default filter adds `bevy_ecs::system::function_system=warn` and
   `bevy_ecs::query::state=warn` (both spans are info level; that module's warnings still log). Log lines lose their
   `system{name=...}:` prefix in normal play; trace-all keeps it (it turns the timing readouts on). `RUST_LOG` still wins.
4. **Menu / HUD movie VM** (`apt_vm.rs`):
   - `Vm::get`'s case-insensitive fallback scanned every key of every object in the prototype chain on each miss. Object
     members are now a `Fields` type (reads through `Deref`, writes only through `insert` / `remove`, no `DerefMut`, so the
     compiler routes every write through it) that counts each key's ASCII-lowercase form; the fallback skips objects with
     no key of the query's lowercase form. `eq_ignore_ascii_case(a, b)` is exactly `a.to_ascii_lowercase() ==
     b.to_ascii_lowercase()`, and matching objects are still scanned in the same order, so the result is the same.
     Test `get_with_folded_key_skip_matches_the_full_scan` (20 000 random inserts / removes / prototype changes with
     mixed-case and non-ASCII keys: equal to the old scan every step; folded counts always match the keys).
   - `Vm::collect` (the HUD movies collect every frame): the mark buffer is kept, slots already freed and empty are not
     rebuilt, the free list is returned by reference instead of cloned. Collection still runs at the same points, so
     object ids are unchanged.
5. **Grab scene prop geometry** (`prop_dynamics.rs`): each grab-scene refresh (twice per tick) built a new `Arc<Geometry>`
   and points Vec per spline of every awake prop. The geometry depends only on the splines, the grab slot and
   `axis_scale`; it is now built once per body (`rebuild_grab_geometry`, called wherever the splines or the slot change;
   `axis_scale` is fixed after creation) and shared. Test builds compare the cache with a fresh build on every call
   (`grab_object_of`), so every prop-grab test checks it.

Proof: the physics, living world, APT, HUD, menu and prop suites with the data-gated tests on the real install: 501
passed; the 8 failures are the two climbing tests that fail the same way without these changes and six that need other
maps; with University for the grind / water / map-startup ones those 5 pass too (the boundary test needs Industrial, the
recorded test an input recording).

Result (both batches against the same base exe, interleaved, no run disturbed):
| | base | new |
|---|---|---|
| with the timing readouts on (spans on in both): frame p90 / p99 | 6.04 / 6.94, 6.00 / 6.79 ms | 5.60 / 6.62, 5.57 / 6.68 ms |
| plain (as in normal play: spans off in new): frame p50 | 3.65 / 4.00 ms | 3.43 / 3.77 ms |
| plain: frame p90 | 5.89 / 6.28 ms | 5.30 / 5.54 ms |
| plain: frame p99 | 6.90 / 7.32 ms | 6.66 / 6.51 ms |
| plain: main thread p50 | 2.62 / 2.81 ms | 2.50 / 2.66 ms |

Laptop (second PC, automated, 2026-11-02): the same 501 passed / 8 known failures; `lw_bench.sh` PLAIN, interleaved,
no run disturbed, hails-local (both batches) vs hails-additions 6bd43e6f:
| | hails-additions | new |
|---|---|---|
| frame p50 | 4.85 / 4.69 ms | 4.32 / 4.47 ms |
| frame p90 | 7.64 / 7.51 ms | 6.39 / 6.51 ms |
| frame p99 | 8.47 / 8.19 ms | 6.96 / 7.12 ms |
| main thread p50 | 3.29 / 3.19 ms | 2.81 / 2.82 ms |
A trace-all run there (spans on in both; first batch only) gave about 0.1 to 0.2 ms at the median and 0.25 ms at p90 / p99.

### Fades: MeshTag
(user 2026-11-02: go ahead.) The fade wrote each fading NPC's / ped's blended material copies every frame
(`present_fade`, StandardMaterial and PedMaterial): 450 to 1500 writes per second per type with 15 peds (peds fade by
camera distance, so one in the fade band changes alpha whenever the camera moves), and each write made Bevy rebuild that
material's bind group and re-run the specialization checks.

Change:
- The copies stay as before (created when a fade starts, per NPC, from the source at that moment, freed with
  `NpcFadeMaterials`), but the per-frame alpha goes to each mesh's `MeshTag` (`fade_tag(base * alpha)` =
  `to_bits() ^ 0x8000_0000`; tag 0 = not fading; -0.0 is drawn as +0.0, the same blend). At alpha 1 the original
  material comes back and the tag is removed.
- Shaders read the same f32 back with `bitcast`: the ped shader (`ped_material.wgsl`, output alpha) and the vendored
  PBR fragment and prepass discard (`vendor/README.md` "Fade alpha in MeshTag"). PedMaterial has no prepass fragment
  shader, so its shadow pass never read the alpha (`prepass/mod.rs` `fragment_required`); StandardMaterial's blended
  shadows discard below 0.05 alpha, so the prepass gets the same substitution.
- Bevy copies the tag into the mesh uniform on both the CPU and the GPU preprocessing path and re-extracts on
  `Changed<MeshTag>` / removal.

Proof: the fade test now checks that the mesh's tag carries the exact bits of the old copy's alpha on every tick and is
removed at alpha 1; living world + replay suites pass (187); the game runs with no shader or pipeline errors, peds render
and fade (`--verify` capture); `MATERIAL_EVENTS` shows `PedMaterial` modifications 450..1300 -> 0 and StandardMaterial
769..1509 -> 0 per second (the CharacterMaterial lighting writes, 160..280 per second, are a separate path and already
skip unchanged values).

Result (plain, interleaved against the second-batch build, 2 runs each, no run disturbed): frame p50 3.65 / 3.63 ->
3.30 / 3.25 ms, p90 5.52 / 5.52 -> 5.60 / 5.40 ms, p99 6.48 / 6.57 -> 6.75 / 6.47 ms, main thread p50 2.59 / 2.60 ->
2.64 / 2.61 ms: the gain is the render side (bind group rebuilds), which held frames back more than the main thread's
numbers showed.

## In-play loading (2026-11-02, USER-ASKS 469)
Question: can assets be pre-loaded / loaded during play more cheaply? Measured per event first (`LOAD_EVENTS` line under
`SKATE_SYSTEM_TIMING=1`: count / total ms / longest ms per second; desktop, release, muted, University library, 150 s):

| event | count | mean | longest |
|---|---|---|---|
| ped tint recolour + new texture (`present_ped_tints`, main thread; plus a 1 MB upload) | 24 | 1.6 ms | 1.9 ms |
| ped rig bind | 24 | 0.05 ms | 0.08 ms |
| NPC skater rig bind | 32 | 0.06 ms | 0.10 ms |
| fade material copy | 416 | ~0 | 0.03 ms |
| DMO prop stream pass | 7401 | 0.01 ms | 28.7 ms (the census's first fill, once per map) |

Audio banks and speech already load on their own threads (emitter prefetch, speech takes). Research inventory:
`.local/research/in-play-loading.md`.

### Ped tint cache
The recoloured ped materials were cached by weak id, so when the last ped with a (source material, tint pair)
despawned its texture went, and the next ped with that pair recoloured and uploaded it again: every ped spawn in the
bench missed. `PedTintKeep` now holds the most recently used ones strongly (`SKATE_PED_TINT_CACHE=<n>`, default 32
entries, about 32 to 64 MB of kept textures; 0 = the old behaviour); entries whose source material is gone (a map
change) are dropped. Same pixels: the recolour is a pure function of the source texture and the tint pair, and the key
is both. Bench with the cache: half of the tint lookups hit. Test `ped_tint_keep_holds_the_most_recently_used_and_evicts_the_oldest`.
Not helped: the start of a map, when the first population (15 peds) binds within a second and every tint misses
(23.6 ms of recolouring in that second).

### DMO first fill: left as is
The first census fill culls about 460 props at once: bake 5.8 ms, triangle replace and index rebuild 5.9 ms, the rest
about 9 ms. It runs on the first fixed tick, just after the loading screen; while a startup hold is up virtual time is
paused, so no tick can run behind it, and running the fill earlier with other inputs (camera, observers) or a first tick
behind the hidden window would not keep the outcome the same.

### The in-play hitches are not loading
The 20 to 48 ms hitches (about one per 10 s in the bench) come just as often with the living world off (19 against 16
to 21 at 20 ms or more in 150 s). With the render frame's parts timed (`SYSTEM_TIMING` now also names bevy_render's
`run_graph` / `node` / `submit_graph_commands` / `present_frames` / `command_buffer_generation_tasks` spans), the
render-side ones sit in `command_buffer_generation_tasks` (Bevy's parallel draw recording on the compute pool), in the
same seconds as `propagate_parent_transforms` on the main thread: the process-wide stall of doc 25 (research b230), a
blocked task picked up by a waiting compute-pool scope. Open; a Windows Performance Recorder trace with context
switches of one stall is the next step there.

## Transform propagation stalls
(2026-11-02, USER-ASKS 469; the doc 25 process-wide stall, research b230.) A Windows Performance Recorder trace with
context switches of one 36 ms stall: the main thread waited 32.7 ms on a compute-pool worker that was not waiting at
all but runnable: preempted on its CPU and kept off it for 32.5 ms by three other compute workers running
`propagate_parent_transforms`' propagation worker at a boosted priority. Bevy 0.18.1 starts one such worker per compute
thread and the idle ones spin (`try_lock` / empty queue -> `continue`) without yielding, so a preempted worker that
still holds work waits a scheduler quantum or two; every scope on the pool (the render thread's command buffer tasks,
the specialization `par_iter`s) waits with it. That is why the stalls cluster at about 33 / 66 ms and come as often with
the living world off.

Change: vendored `bevy_transform` 0.18.1 (`vendor/README.md` "Transform propagation workers yield"): an idle worker
drops the queue lock and calls `std::thread::yield_now` (Windows `SwitchToThread`: another ready thread of that CPU
runs) after 64 failed polls. The work, and which entity's GlobalTransform is computed from which parent, are unchanged.

Proof: skate-game 991 pass, 235 ignored; data-gated living world and replay pass (the NPC golden digests among them).

Result (desktop, plain, interleaved against the build before, 2 runs each of 150 s, no run disturbed): frames of 20 ms
or more after the start 22 / 12 -> 1 / 1; per-second worst frame p90 27.1 / 28.7 -> 11.0 / 9.6 ms; p99.9 19.5 / 17.8
-> 19.5 / 9.2 ms; median 3.42 / 3.35 -> 3.62 / 3.43 ms and p90 5.69 / 5.64 -> 6.16 / 5.74 ms (0.1 to 0.2 ms at the
median across two different builds).

Tuning (one build, `SKATE_TRANSFORM_YIELD_SPINS` only, interleaved, 2 runs of 150 s each):
| spins before the yield | frame p50 | p99 | p99.9 | frames of 20 ms or more after the start |
|---|---|---|---|---|
| 0 (upstream: never yield) | 3.46 / 3.87 ms | 7.17 / 8.56 ms | 21.8 / 16.0 ms | 27 / 8 |
| 64 (default) | 3.31 / 3.35 ms | 6.73 / 6.74 ms | 9.0 / 8.9 ms | 1 / 0 |
| 512 | 3.56 / 3.31 ms | 7.57 / 7.03 ms | 9.2 / 9.3 ms | 1 / 0 |
64 is the best in every column, the median included: the earlier median difference was between two builds, not the
yields' cost.

### Checked and left alone
- `think_peds` builds a String per entity per tick for its entity map: about 1200 small allocations a second, too small to
  matter.
- `sync_npc_worker` copies only small per-tick lists; the props are shared behind an `Arc`.

### Open leads
- `refresh_grab_props` still rebuilds the grab scene registry twice per tick (player step and NPC phase); skipping the
  second needs proof that nothing moves the props in between.
