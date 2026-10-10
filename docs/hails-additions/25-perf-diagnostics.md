# 25. Performance: frame-time diagnostics and faster setup

Branch `feature/perf-diagnostics` (from `main` 4488651). One upstream PR, two parts:

1. **Frame-time diagnostics** (todo `frame-timing`): a frame-time counter that shows stutter, a per-frame log
   behind `SKATE_FRAME_LOG`, a graphics-menu row to turn the counter on, and a read-only `sdk.snapshot.frame`
   for mods. Observation only: the game is identical with all of it on or off.
2. **Setup speed, phases 4–5** (todo `asset-pipeline-performance`): setup runs below normal priority with
   overridable worker counts, and a map's conversion overlaps its independent work (texture and blob
   compression on threads, movable props beside the collision archive and the map writer). Every output is
   byte-identical.

Machine for all numbers: Windows 11, i7-14700KF (20 cores / 28 threads), Python 3.13.

---

## Part 1: frame-time diagnostics

### Problem

The user: "the ingame fps counter is not the best". A session felt "super stuttery when I try to do tricks"
(2026-10-02) and nothing could measure it:
- The on-screen counter (`fps_overlay.rs`) showed `FPS: N` = frames / seconds over 0.5 s windows. An average
  hides a hitch: 167 frames of 3 ms plus one 200 ms frame read **240 FPS** (unit test
  `one_hitch_is_visible_where_an_average_hides_it`). It was always on.
- The audio state log runs on the fixed 60 Hz step, which catches up after a hitch, so it shows 60 rows/s while
  rendering stutters.
- The game log has no frame times.

### Root cause of the misleading counter

It measured the right clock (`Time<Real>`, the presented frames) but reported only a 0.5 s mean. The worst frame,
the lows and the hitch count are what describe stutter; a mean cannot.

### Change

`crates/skate-game/src/frame_timing/` replaces `fps_overlay.rs`:
- `stats.rs` (pure functions, unit-tested): nearest-rank percentiles, the window summary (mean, median,
  **1 % low** = 99th-percentile frame time, **0.1 % low** = 99.9th, worst), the hitch rule (a frame longer than
  **2× the median of the previous 120 frames**, needs 30 frames of history), and the graph buckets (worst frame
  per 50 ms bucket, so a hitch always shows as a bar).
- `mod.rs`: the `FrameTiming` resource. A frame is one iteration of the app's main loop, timed with its own
  clock from the start of `First` to the start of the next `First` (what the player sees; in steady state the
  main loop waits for the pipelined render thread, so this is the presentation interval). One push per frame
  into a 5 s window (bounded at 20,000 frames); the summary is recomputed 4× per second. Systems:
  - `begin_frame` (`First`, before the time update): closes the previous frame and pushes / logs it.
  - `begin_fixed` / `end_fixed` (`RunFixedMainLoop`, before / after the fixed loop) and `count_fixed_step`
    (`FixedFirst`): the physics part of the frame.
  - `record` (`Last`): the frame's CPU split: `main_ms` (its First..Last schedules), `fixed_ms`, `fixed_steps`.
    A frame much longer than its `main_ms` waited outside the schedules (render thread, GPU, present, OS). The
    fixed steps that catch up after a long frame run in the following frame(s): the virtual clock advances
    from the render thread's timestamps.
    - Why not `Time<Real>`'s delta: with pipelined rendering it comes from the render thread and lags the main
      loop by a frame; a first version paired it with the CPU split and produced rows with `main_ms` >
      `frame_ms` (smoke run). With the own clock every row satisfies `main_ms <= frame_ms`.
  - `show` (`Last`): the overlay (top right): `FRAME ms / fps` over the last 0.25 s, `1% low`, `0.1% low`,
    `worst 5 s`, `hitches`, `CPU main / physics`, and an 80-bar graph (4 s; green ≤ 16.7 ms, yellow above, red =
    hitch). Text updates at 4 Hz, the graph at 10 Hz; hidden = no UI work.
  - `SKATE_FPS_LOG` still prints the old `SKATE_FPS_SAMPLE` lines.
- `log.rs`: `SKATE_FRAME_LOG=<file>` → one TSV row per frame, written by a background thread with a bounded 4,096-row queue. Sending never blocks: a full queue drops
  new rows, and the dropped count is reported on shutdown. The writer flushes after draining each batch
  and on drop, and is joined on drop. Unset: no thread, no file, no row work. Format v1:
  ```
  # skate3rust frame log v1
  wall_unix_s  frame  frame_ms  fixed_steps  fixed_ms  main_ms  hitch  median_ms
  ```
  `wall_unix_s` (UTC) lines rows up with `logs\game-*.stderr.log` and the audio state log; `hitch` is `HITCH` or
  empty. Reading tool (fork-local): `.claude/skills/optimisation/tools/frame_log_summary.py`.
- `graphics_menu.rs`: row 28 "Frame-time counter On/Off" in GRAPHICS (after the audio controls), saved as
  `frame_stats` in `settings/graphics.json`, **off by default** (old files load as off).
- Moddability: `sdk.snapshot.frame` (read-only): `frame, ms, fixed_steps, main_ms, fixed_ms, hitch, window_s,
  frames, fps, mean_ms, median_ms, low_1_ms, low_01_ms, worst_ms, hitches`. Defaults (zeros) in the Lua VM so it
  is never nil; documented in `sdk/ENGINE_API.md` ("Frame-time statistics") and `sdk/skate.lua`
  (`FrameSnapshot`). Mods cannot write it.

### Retail parity

Diagnostics only; nothing in retail Skate 3 to match. The proof obligation is that gameplay is unchanged:
- `systems_touch_only_their_own_state`: from Bevy's system access sets, no frame-timing system writes any
  resource but `FrameTiming`, none writes everything, and only `show` writes components (its overlay nodes).
- `game_is_identical_with_diagnostics_on_or_off`: a Bevy app with a fixed-update "game" (60 Hz, stateful)
  through 600 uneven frames (incl. 120 ms hitches), without the plugin, with it, and with the log: the fixed
  steps, every fixed delta and the state (bit pattern) are identical. Every fixed step is attributed to a logged
  frame (only the final frame's split is still pending).

### Tests

`cargo test -p skate-game --release --bin skate3rust -- frame_timing graphics_menu`, plus skate-mods
`frame_statistics_have_defaults_and_keep_host_values`. Listed in "Verification" below.

### `FRAME_HITCH` log line (2026-10-08)

Problem: the user reported "considerable frametime lags while flipping in the air on large jumps", then
"yes on every large jump when flipping" (session 2026-10-07 13:09, spillway after the observatory). The
game log had no frame times (the counter only drew the overlay, `SKATE_FRAME_LOG` was off), so the hitch
could not be placed or attributed afterwards.

Change: `frame_timing/hitch.rs` writes one `FRAME_HITCH` line per hitch frame, always on, rate-limited:
frame ms, median, main-thread CPU, physics loop ms and steps, wait outside the schedules (render thread,
GPU, present, OS), the game thread's wait for the audio render lock, the wall time of the stages (fixed
input / controls / physics; frame assets / present / animation / audio pass), the phases inside the
physics tick (animation graphs, collision and solve, finish, scoring) and the scoring HUD (fixed advance,
frame render), the slowest of them, the player (state, airborne, wheels, body flip, trick name and
sequence, board position, tick), the entity count and the audio load (mixer voices and AEMS instances of
the last rendered block), plus how many hitch frames the rate limit skipped.

- Thresholds are data with defaults (`HitchConfig`: factor 2.0 x median, floor 50 ms, at most one line
  per 0.5 s), overridable without a rebuild: `SKATE_FRAME_HITCH=off` or
  `SKATE_FRAME_HITCH=factor=2,floor_ms=50,interval_s=0.5`. No mod tuning path exists for the frame
  diagnostics (the snapshot's `frame` section is read-only), see open questions.
- Observation only: the marker systems write only `HitchSpans`; the phase timers are relaxed atomics
  around existing calls; the audio counters are two stores per rendered block and one clock pair per game
  thread lock. Test `systems_write_only_their_own_state`, and the existing
  `game_is_identical_with_diagnostics_on_or_off`.

Measurements so far (headless, 2026-10-08, optimised test build):

- `flip_hitch_timing` (new, data-gated, `tests/flip_hitch_timing.rs`): scripted ollies, kickflips,
  heelflips and late flips through the real `frame::advance`, on normal airs and on large airs (upward
  speed added on the first airborne tick, ~5 s of air), first use and repeated. Worst physics tick 2.8 ms,
  median about 1 ms; animation graphs at most 0.5 ms, scoring 0.0 ms. No tick spikes on a flip, first or
  repeated. Stock clips decode lazily on first use (`animation_frames/native.rs`), but that costs well
  under a millisecond and happens once per clip, so it cannot cause a hitch on every flip.
- `e2e_render` with `E2E_TIMING=1` on 70 s of the 13:58 session's audio state log with 12 airs of 1.2-2.2 s
  with spins (rows 77000-81200 of `state_20261007_135825.tsv`), all player layers including tricks and
  treatment: game-thread audio at most 0.18 ms per frame, render at most 0.5 ms per 256-frame block
  (budget 5.3 ms). A ground-riding window of the same session gives the same numbers. The trick and
  treatment banks are loaded and decoded at boot (`native.rs` `load_optional_player_banks`), so a flip
  reads nothing from disk.
- So the player's physics, animation graphs, scoring and player audio are not the cause. Not measurable
  headlessly: the frame-side animation present / skinning, the camera, the HUD render and the renderer;
  the `FRAME_HITCH` line separates those in the next session.

### Scoring HUD asset churn (2026-10-08, branch `world/hud-hitch`)

Problem: lead for the flip hitch: the phases named for it include `hud_advance` and `hud_render`, and the
scoring HUD looked like it rebuilt its meshes and materials every frame.

Root cause found in the code: `scoring_hud::render` does keep one retained slot (entity, mesh, material)
per draw, so it does not add new assets per frame. But for every slot, every frame, it called
`Assets::get_mut` on the mesh and the material and replaced them, and inserted `Visibility` on every slot
entity. `get_mut` marks the asset modified whether or not anything changed (the same trap the HUD target
resize hit earlier), so every HUD glyph and shape was re-extracted, its vertex buffer re-uploaded and its
material bind group rebuilt in the render world every frame. The number of draws grows with the trick text
on screen, so the churn is highest exactly while a trick name and score are shown in the air.

Evidence (headless, optimised test build, the real scoring runtime and the real HUD movie;
`scoring_hud::draw_tests::hud_writer_is_identical_and_skips_unchanged_assets`, 2400 frames: ten 2.5 s airs
with three flips each, multiplier 3 from the second air, a bail; two runs, same numbers):

| | old writer | new writer |
|---|---|---|
| Mesh `Modified` events per frame (mean) | 21.9 | 1.3 |
| Material `Modified` events per frame (mean) | 21.9 | 2.3 |
| Worst frame (mesh + material events) | 80 (40 draws on screen) | 50 (frames where the trick text changes) |
| Writer cost, main thread, p50 / p99 | 9.8-10.1 / 19.1-19.6 us | 4.4 / 22.6-23.9 us |

Other main-thread HUD costs in the same run: `hud_runtime::update` (the `hud_advance` phase) p50 15 us,
p99 130 us; `apt_scene::draw` p50 20 us, p99 35 us. Maxima (35-260 us) moved between runs and are noise.
So on the main thread the HUD costs well under a millisecond with either writer and cannot by itself
make a frame twice the median; `hud_advance` / `hud_render` naming a hitch would point elsewhere. The cost
the fix removes lands in the render world (extract, mesh allocator, bind groups: up to 80 asset
re-preparations per frame on trick frames), which headless tests cannot time; in a `FRAME_HITCH` line it
shows as `wait` outside the schedules, not as `hud_render`.

Change (`crates/skate-game/src/scoring_hud.rs`): `apply_draws` writes a slot's mesh only when its
positions, UVs or normals differ bit for bit (`to_bits`, so -0.0 / +0.0 and NaN payloads count as
changes) from what the mesh already holds, its material only when the colour transform bits or the atlas
handle differ, and its `Visibility` only when it differs from the value it last inserted (`Slot::visibility`,
`None` until the first insert so a new slot keeps the spawn default `Inherited` for its first frame, as
before). New slots are spawned exactly as before. Slot order, z, render layer and the panic-free
behaviour on a missing texture are unchanged. The main-thread p99 is about 4 us higher (the comparison
runs before a rebuild on frames where a slot changes); traded for removing ~40 render-world re-uploads per
frame on average.

Verification:
- Identity: the old writer is kept verbatim as a test-only reference; the test drives both with the same
  draws every frame and asserts, after every frame, that every slot is identical: entity, `Visibility`,
  `Transform` z bits, `RenderLayers`, mesh and material handle bindings, topology, indices, asset usage,
  every attribute's bytes, colour transform bits and atlas texture. Passes on all 2400 frames, twice.
- `skate-game` suite: 561 passed, 1 failed in the binary (`setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`,
  the known pre-existing failure), the existing `hud_target_changes_only_on_resize_and_refreshes_composite` passes.
- Retail parity: the HUD look cannot change; the render world receives the same asset contents, only
  fewer redundant change notices. Moddability: mods only read the HUD's trick names
  (`modding::observation`); no mod writes HUD geometry, so there is nothing to reset on mod disable.
  Deterministic, no networking.

Open questions (need a user session with `FRAME_HITCH`, or trace-all with `SKATE_PERF_RENDER`):
- Whether this churn was the flip hitch: compare `wait` and the render phase split (`PrepareAssets`,
  `PrepareMeshes`) on large flips before and after this branch.
- If hitches remain with `hud_render` or `hud_advance` as the slowest phase, the cause is not their own
  cost (sub-millisecond headless); look at what the frame waited on.
- `hud_runtime::update` p99 130 us is the largest HUD cost left; not a hitch, not changed here.

### Follow-up: Simple and Verbose modes

The full readout covers a lot of the screen for everyday play. The GRAPHICS row "Frame-time counter" now cycles
Off / Simple / Verbose instead of Off / On.
- Verbose is the readout above, unchanged.
- Simple: fps, frame time, 1 % low and worst frame on two short lines in a smaller font (11 px instead of 14 px),
  with a small spike graph of the last 2 s (40 bars, 14 px high).
- Saved as `frame_stats_simple` next to `frame_stats` in `settings/graphics.json`. Files saved before this
  change have no such field and open as Verbose when the counter was on, so nobody's counter changes by itself.
- Files: `frame_timing/mod.rs` (`readout_simple`, layout switch in `show`, done once per mode change),
  `graphics_menu.rs` (setting, row cycle, label).
- Test: `frame_counter_setting_defaults_off_and_round_trips` also checks the old file and a Simple round trip.
- Diagnostics only; the same proofs as above apply (only the overlay nodes change).

---

## Part 2: setup speed, phases 4–5

### Phase 4: priority, worker counts, overrides

New module `tools/asset_pipeline/setup_budget.py` (its name matches no fingerprint glob on purpose):
- `install()` runs setup at **below-normal priority** (`lowered_priority`, restored afterwards) and `run()` starts
  every conversion process (map jobs, the game's `--check-assets`, extract-xiso) with
  `BELOW_NORMAL_PRIORITY_CLASS`; their children inherit it.
- Overrides: `SKATE_SETUP_PRIORITY=below_normal|normal|idle`, `SKATE_SETUP_MAP_WORKERS=<n>`,
  `SKATE_SETUP_THREADS=<n>` (threads inside one map job).
- Map workers unchanged: `min(3, cpu/2)`, capped at (free RAM − 2 GiB) / 3 GiB. **Measured peaks** (2026-10-04):
  DownTown's conversion job 716 MiB working set / **1.44 GiB committed**; the game's `--check-assets` that
  follows peaks at **1.73 GiB working set / 1.9 GiB private** (University 1.33 / 1.41, Industrial 0.96 / 1.06).
  The 3 GiB per-worker budget therefore holds for the largest district. More than 3 workers barely helps: DownTown
  alone sets the map stage's length.

### Phase 5: inside one map

Measured on DownTown with `main` (`.local/perf-bench/map_bench.py`, validation stubbed): extract 11.5 s (cold
disk) / 1.0 s (warm), prepare 122–128 s, collision archive 2.7 s, write_map 26.8–29.2 s, props 8.5–10.6 s,
hash + cleanup 2.4–3.1 s. (`prepare` is dominated by stream decoding, which PR G #29 cuts; not part of this PR.)

1. **write_map compresses on threads** (`map_writer.py`):
   - `write_textures(output, root, textures, workers=None)`: the ordered window is now `job_threads()` wide
     (default cpu/4, 2..6; was 2). Writes stay in name order.
   - The vertex, index and empty blobs and the RWCM / WMET extension blobs do not depend on the textures; they are
     compressed on two more threads while the textures are written, and written in the original order.
     `stored()` is now `packed_blob()` + write (same bytes).
   - DownTown write_map (same intermediate, A/B): 2 texture threads 18.1–18.6 s, 3 → 15.0 s, 6 → 12.8–12.9 s;
     main 18.6–19.3 s.
2. **Movable props beside collision + write_map** (`install.convert_map`): the props only read the prepared
   manifest and the DMO catalog and write their own folder, so they run on one background thread from the end of
   `prepare`. `props` is now the time left waiting for them (0.0 s on DownTown). Error handling unchanged (a
   `CONTENT_ERRORS` failure still writes `<map>-availability.json` and the map still converts).

Result (all districts, below). DownTown collision + write_map + props: **37.9 → 24.4 s**.

Rejected, with numbers:
- **Extract without writing the stored BIG entries**: the 11–15 s "extract" was the cold read of the 843 MB
  archive. Warm, extracting DownTown takes 0.7–1.4 s (threaded 0.3–0.4 s), so not writing would save ≤ 1 s and
  needs the vendored stream loader to read from the archive (fingerprint of every map group). Not done.
- **Prefetching model npz files on threads in write_map**: 15.8 → 18.1 s (slower). NpzFile header parsing is
  pure Python and holds the GIL.
- B5G6R5 decoded twice / collision decode allocating per triangle: both in vendored parsers (`PARSERS`, i.e. the
  character, environment and maps fingerprints). Left open.

### Equivalence (byte-identical)

- **Every district, every output.** `.local/perf-bench/all_maps.sh` converts each of the 10 districts with
  `main`'s tools (`git archive main tools`) and with this branch, alternating per map, into the same scratch
  stage path (the WMET block embeds the stream path), and hashes every output: `maps/<map>.skate`,
  `maps/<map>.irradiance`, `native-props/<map>.skate/*`. **ALL IDENTICAL** (10/10 districts, 3–4 files each),
  including the map entry (`sha256`). Validation was stubbed in both (not under test).

  | District | collision + write_map + props, main → branch | Peak committed memory |
  |---|---|---|
  | DownTown | 37.9 → 24.4 s | 1443 → 1445 MiB |
  | University | 29.9 → 21.1 s | 1273 → 1273 MiB |
  | Industrial | 26.1 → 16.5 s | 1165 → 1167 MiB |
  | SkateSchool | 3.8 → 2.2 s | 832 → 831 MiB |
  | MaloofMoneyCup | 1.9 → 1.1 s | 825 → 845 MiB |
  | DownTownSkatePark | 1.5 → 0.9 s | 809 → 821 MiB |
  | BlackBoxPark, IndustrialSkatePark, MegaPark, StartPark | 0.4–0.7 → 0.2–0.4 s | ≤ +22 MiB |

- **Environment group** (the other user of `map_writer.write`): `backdrop.convert` with both trees → the 3
  backdrops (`DownTown`, `Industrial`, `University`) byte-identical.
- **Props failure path**: with an empty DMO catalog (StartPark), both trees write the same
  `StartPark-availability.json` status / error; only the traceback text differs (file path, line, and the
  function name `movable_props`).
- **Thread count**: `write_map` on the same DownTown intermediate with `SKATE_SETUP_THREADS` = 2, 3, 6 and with
  `main`: all byte-identical (`phase_bench.py`).

### Fingerprints

`map_writer.py` (maps + environment) and `install.convert_map` (maps) changed. Pairs in
`tools/asset_pipeline/pipeline-equivalence.json` for exactly these two groups, from `main`'s fingerprints to this
branch's: environment `e062aa4b…` → `39097274…`, maps `2d52e21d…` → `81867db8…`; `changed_groups(main, branch)` is
empty, so an existing installation rebuilds nothing. core, hud, character: unchanged. Customiser fingerprint and
stage versions: unchanged (checked against `git archive main tools`). `setup_budget.py` is in no fingerprint;
`install.py` gained no top-level import. When this PR and #28 / #29 (which also change the maps fingerprint) are
combined, the pairs have to be recomputed for the combined tree (as for F + G).

## Verification

- Rust: `cargo test -p skate-game --release --bin skate3rust --locked`: 323 passed, 2 failed - the known
  pre-existing `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes` and
  `retail_render::shader_tests::sky_shader_validates`. New: 18 in `frame_timing` + `graphics_menu` (stats math,
  log format and writer, window, CPU split, app-level identity, system access, menu setting).
  `cargo test -p skate-mods --release --locked --lib`: 58 passed (new `frame_statistics_have_defaults_and_keep_host_values`);
  the `skyline_physics` integration test needs the Skyline GLB, which is not in a fresh checkout.
- Python (`py -3.13 -m unittest`): `test_map_writer` (whole-map identity across thread counts and render-only,
  blob layout, texture workers 1/2/3/7), `test_setup_budget` (overrides, priority classes, a child process really
  starts BelowNormal and the previous class returns, `run()` creation flags), plus `test_versions`,
  `test_setup_assets`, `test_setup_refresh`, `test_setup_recovery`, `test_dynamic_props`, `test_irradiance`: all OK.
- Smoke runs (muted is not available on `main`; `SKATE_REPORT_CHILD=1`, `--verify`, University, a settings copy
  with the counter on): the overlay renders (screenshot), the log is written (v1 header, 480–2,047 rows), no
  panics or errors; `frame_log_summary.py` reads it; every row has `main_ms <= frame_ms`.
- Full fresh setup into a scratch base (`.local/perf-bench/full_setup.py`, `default.xex`, 3 map workers), `main` vs
  branch, run back to back: total **445.6 → 434.6 s**; map stage 183.1 → 179.8 s (DownTown is the long pole and its
  `prepare` stream decode, cut by #29, dominates); customiser 228 → 234 s (unchanged code; runs after the maps on
  `main`). Both: 0 warnings. Single runs on a shared machine: the per-district A/B above is the reliable number.
  The full-setup gain is small on `main` because `prepare` dominates; with #29 the post-prepare phases are a larger
  share of each map.

## Files

- `crates/skate-game/src/frame_timing/{mod,stats,log}.rs` (new; replaces `fps_overlay.rs`), `main.rs`, `app.rs`,
  `graphics_menu.rs`, `modding/mod.rs`; `crates/skate-mods/src/vm.rs`; `sdk/ENGINE_API.md`, `sdk/skate.lua`.
- `tools/asset_pipeline/setup_budget.py` (new), `install.py`, `map_writer.py`, `pipeline-equivalence.json`;
  tests `test_setup_budget.py` (new), `test_map_writer.py`.
- `FRAME_HITCH`: `frame_timing/hitch.rs` (new), `frame_timing/mod.rs`, `physics/frame.rs`
  (phase timers), `scoring_hud.rs` (HUD phases), `game_audio/timing.rs` (voice and lock counters),
  `game_audio/mod.rs` (`timing`, `CueSet` visible to the crate), `tests/flip_hitch_timing.rs` (new),
  `physics.rs` (test module).

## Open questions

- Not yet run on the branch: regression-check `check_maps.py` / `check_spawns.py` against the scratch install
  (outputs are byte-identical to `main`'s, so they can only match `main`).

- Per-system split beyond main / physics / render-wait (e.g. audio, streaming): use Bevy's tracing spans
  (`TRACE_PLAY.bat`) for that; not built into the counter.
- Phase 5 leftovers in vendored parsers (above), and `hash_and_cleanup` (~3 s, I/O).
- Default on/off of the counter: off (todo); the user may prefer on.
- `FRAME_HITCH`: the cause of the flip hitch on large airs is not found yet (the scoring HUD asset churn, removed on `world/hud-hitch`, is the first candidate, see above); the next session with the line
  names the stage. Thresholds are env-tunable only; a mod-facing setter (and reset on mod disable) would
  need a writable frame-diagnostics section in the mod API.

## Trace-all mode (`SKATE_TRACE_ALL=1`)

Status: done on `world/trace-all` (not yet played).

Problem: one play session should capture every trace the engine has, and the name has to mean what it says. Before, trace-all only meant whatever the launcher remembered to set, and two of those switches were unsafe for a whole session: `SKATE_PERF_REPORT` exits the game after 25 s, and `SKATE_GPU_TIMING` makes device creation fail on an adapter without timestamp queries.

Change (`crates/skate-game/src/trace_all.rs`): `apply()` runs first in the game process, before any thread or the log subscriber, and sets every session diagnostic switch that is not already on. Every existing reader (including the `OnceLock` caches in audio and physics) then sees it unchanged. Paths the launcher did not set go next to the ones it did set (fallback `<exe dir>/logs/trace-all-<unix seconds>`). One `TRACE_ACTIVE` startup line lists every active trace and where it writes.

| Switch | Output |
|---|---|
| `SKATE_FRAME_LOG` | `frames.tsv`, one row per frame (writer thread) |
| `SKATE_AUDIO_STATE_LOG` | `audio_state.tsv`, one row per audio frame (writer thread; rows are now copied as values and formatted on the writer thread) |
| `SKATE_PERF_REPORT` | `perf.json`, rolling: 15 s windows for the whole session, rewritten by the `perf-report` thread after every window (write then rename), one `SKATE_PERF window=` line per window; never exits |
| `SKATE_PERF_GPU` | render diagnostics in the report (GPU pass times when supported); in trace-all the GPU queries run on one frame in 30 (`SKATE_PERF_GPU_EVERY`, see [GPU queries sampled](#gpu-queries-sampled-skate_perf_gpu_every)) |
| `SKATE_PERF_RENDER` | render phase split in the report, reset per window |
| `SKATE_GPU_TIMING` | in trace-all the features are not forced: Bevy's default `Functionality` priority already requests every feature the adapter has, so timestamps are on when supported and device creation cannot fail; `GPU_TIMING timestamp_query=yes/no` logged once |
| `SKATE_AUDIO_TRACE` | log: `AUDIO_NATIVE` post/release, `AUDIO_EVENT` brake/push/grind |
| `SKATE_AUDIO_TIMING` | log: audio cost once per second |
| `SKATE_LIVING_WORLD_DEBUG` | log: population / NPC / traffic readout every 5 s |
| `SKATE_FPS_LOG` | log: `SKATE_FPS_SAMPLE` lines |
| Chrome trace (`--trace`) | armed for F9 / F10 to `chrome-trace.json` next to the report (nothing timed or written until F9; field-less spans no longer allocate a label) |

Not included on purpose: test or tool switches (`SKATE_BUDGET_MAP`, `SKATE_VERIFY_*`, `SKATE_RETAIL_*_VECTORS`, data roots) and switches that change the game (`SKATE_AUDIO_MORE_AUDIBLE`, `SKATE_FIXED_EXPOSURE`). `SKATE_AEMS` (still set by the launcher) has no reader any more.

Log writer: with trace-all every log line goes through a bounded queue (16384 lines) to a `log-writer` thread instead of a blocking stderr write on the calling thread (the crash supervisor relays stderr through a pipe). A full queue drops and counts (`LOG_DROPPED`), never blocks. A panic or clean exit waits for the queue to drain first.

`SKATE_PERF_RENDER` cost: the seven marker systems sit between render sets that Bevy already chains (`ExtractCommands, PrepareAssets, PrepareMeshes, ManageViews, Queue, PhaseSort, Prepare, Render, Cleanup`), so they add no ordering beyond what exists; each does one atomic swap.

New or changed log lines (always on unless noted, all edge-triggered or rate-limited):
- `MANUAL_LANDING` per landing (ported from the hails-only line), now with `engage_time` (the motion graph's `ManualEngageTime` intent at touchdown), `graph_state` (motion graph state id and name) and `clip`.
- `PROP_HELD from=... to=...` on every grab / release edge; `HELD_PROP` at 5 per second in trace-all (1 per second otherwise).
- `BOARD_POSSESSION hold / let_go` on the board hand edges (board dropped to grab a prop).
- `RETAIL_MATERIAL_FAMILIES` once per material table load (family counts, 15 = dynamicobject D9).
- `WORLD_SHADOW_FLOOR rgb=[...] retail=true/false` at startup and whenever a mod changes the floor (car shadows under bridges).
- `GPU_TIMING timestamp_query=yes/no ...` once at startup (trace-all or `SKATE_GPU_TIMING`).

Already diagnosable and left as they are: `AUDIO_LANDING`, `AUDIO_EVENT body impact` (a silent fall shows as air / bail / region impact columns in the audio state log with no body impact line), `NPC_SKATER_BACKWARDS`, `PED_*`, `FRAME_HITCH` plus the frame log.

### Measured overhead

`frame_timing::tests::trace_all_overhead` (`--ignored --nocapture`): a 3000-frame headless app with the always-on frame timing plugin, against the same app plus the trace-all per-frame work on the game thread (frame log row, rolling perf sample and window handover), best of 7 interleaved runs, two runs:

| Run | off (us/frame) | on (us/frame) | delta |
|---|---|---|---|
| 1 | 28.85 (spread 3.17) | 34.10 (spread 4.50) | 5.25 us |
| 2 | 28.76 (spread 2.52) | 35.27 (spread 2.81) | 6.52 us |

About 6 us per frame, 0.04 % of a 16.7 ms frame and inside the run-to-run spread. `game_audio::state_log::tests::state_log_row_cost`: formatting a state log row costs 1.81 us; the game thread now only copies it into the queue, 0.03 us. Log lines in trace-all cost one small buffer and a `try_send` on the calling thread instead of a stderr write. GPU timestamp query cost cannot be measured headless (needs the game window); it is only on when the adapter supports it.

### Tests

- `trace_all::tests` (every switch turned on, launcher paths kept, already-on switches left alone).
- `frame_timing::tests::game_is_identical_with_trace_all_on_or_off`: the fixed-step game is bit-identical with the trace-all per-frame diagnostics, and the rolling report writes windows instead of exiting. The existing `game_is_identical_with_diagnostics_on_or_off` still passes.
- Full `skate-game` suite: 563 passed, 1 failed (`setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`, the known pre-existing failure).

### Files

`crates/skate-game/src/trace_all.rs` (new), `main.rs`, `app.rs`, `performance.rs`, `profiling.rs`, `game_audio/state_log.rs`, `physics/manual_landing_log.rs` (new), `physics.rs`, `physics/prop_dynamics.rs`, `physics/offboard/board_manager.rs`, `retail_render.rs`, `frame_timing/mod.rs`.

### GPU queries sampled (`SKATE_PERF_GPU_EVERY`)

Status: done in the `world/living-world` worktree (2026-10-08), uncommitted, not yet played.

Problem: after a trace-all session (University, 2026-10-08 09:58) the user said "ooooof the fps and lag on the trace all is ROUGH." The perf report showed every window at a 33.6 ms median (p95 about 34.6 ms) with the main schedule at about 9 ms, physics about 3 ms, render CPU about 3 ms and the GPU passes about 9.5 ms in total. The suspect was the per-pass GPU timestamp and pipeline statistics queries that `SKATE_PERF_GPU` adds (Bevy `RenderDiagnosticsPlugin`) on every pass of every frame.

Root cause of the 33.6 ms: not the queries. The settings file every launcher version shares (`data/installations/<id>/settings/graphics.json`, written 2026-10-07 23:20) has `"fps": 30`, the graphics menu's FPS limit, and `graphics_menu::pace` sleeps every frame up to 1/30 s. The game runs with `PresentMode::AutoNoVsync`, so nothing else locks the frame rate. The session's frame log agrees: frame median 33.56 ms against a main thread median of 8.76 ms, and `AUDIO_TIMING frame=33.6 ms`. The missing ~24 ms per frame is the limiter's sleep. Turning Esc > GRAPHICS > FPS limit back to Off brings the frame rate back.

The queries still cost something, so they are sampled now. Bevy's readback is asynchronous (`map_async`, collected on a later frame), so the queries never stall the CPU; their cost is GPU work and resolve copies on every pass.

Change (`crates/skate-game/src/performance.rs`):
- `SKATE_PERF_GPU_EVERY=<frames>`: how often the GPU queries run. Default 30 in trace-all, 1 (every frame, the old behaviour) for the one-shot `SKATE_PERF_REPORT` benchmark; 0 turns the GPU queries off while every other trace stays on. A launcher version or user can set it like any other switch (`versions.json` `env`). The value is logged at startup (`SKATE_PERF_GPU sample_every=`) and written into the report (`gpu_sample_every_frames`).
- Bevy's render system only records GPU queries while its diagnostics recorder resource is in the render world (it removes it, runs the graph, puts it back). A render world system right before `RenderSystems::Render` moves the recorder aside on frames that are not sampled and back on sampled ones. The recorder type is crate-private in bevy_render, so it is named through the public `RenderContext::new` signature; no vendoring.
- Bevy smooths render diagnostics over about 0.1 s, so with every frame sampled the report already held roughly the newest frames; with one frame in 30 it holds the newest sampled frame. Readbacks in flight are collected on the next sampled frame, so values arrive up to one interval late.
- Nothing else changes: frame log, audio state log, perf windows, render phase split and log lines are written by the same code as before.

Measured (release build, same build and scene for every mode: University spawn, idle, muted, windowed 1280x800, FPS limit off through a separate settings folder, RTX 4080 SUPER, Vulkan; 60 s per run, first 25 s dropped; two rounds, the second in reverse order; tool `.local/research/traceall-gpu-bench.ps1`):

| Mode | Round 1 frame ms median / p95 | Round 2 frame ms median / p95 | Main ms median (r1 / r2) |
|---|---|---|---|
| no trace (frame log only) | 2.46 / 4.67 | 3.27 / 5.94 | 2.03 / 2.56 |
| trace-all, GPU queries off (`EVERY=0`) | 2.60 / 4.86 | 2.79 / 4.99 | 2.10 / 2.21 |
| trace-all, sampled (default, 30) | 3.54 / 5.99 | 2.95 / 5.28 | 2.72 / 2.32 |
| trace-all, every frame (`EVERY=1`, old) | 3.83 / 6.20 | 4.01 / 6.29 | 2.81 / 2.92 |

Every-frame queries cost about 1.2 ms per frame here (median 3.92 against 2.70 with them off, both rounds averaged), about 40 % of a frame in this light scene. Sampled costs 0.16 to 0.94 ms over off, inside the run-to-run spread (the no-trace runs differ by 0.8 ms between rounds). At the user's 30 fps limit none of this was visible; it matters once the limit is off.

Identical traces: in all six trace-all runs the audio state log has the same 88-column header and 0 malformed rows, the frame log the same header and 0 malformed rows, and `perf.json` the same keys with 3 windows. Sampled and every-frame reports both hold the 17 `elapsed_gpu` entries. The sampled report lists 76 render diagnostics against 82: six invocation counts of the transparent 2D/3D passes were zero on every sampled frame, and the report already leaves out zero values. Use `SKATE_PERF_GPU_EVERY=1` to catch passes that only run now and then. Tests: `performance::tests::gpu_sample_interval_defaults_and_overrides`, `performance::tests::gpu_gate_parks_and_restores_the_recorder` (the recorder is never lost or doubled), and the existing `frame_timing::tests::game_is_identical_with_trace_all_on_or_off` / `..._with_diagnostics_on_or_off` pass (28 passed in `performance::`, `frame_timing::`, `trace_all::`).

### Open questions

- The Chrome trace is armed with F9 / F10 on the keyboard only; a couch session with a pad will not start it.
- The launcher still sets `SKATE_AEMS=1`, which nothing reads.
- `BOARD_POSSESSION` lines come from the shared board controller; if NPC or remote skaters drive it the line has no owner field yet.
