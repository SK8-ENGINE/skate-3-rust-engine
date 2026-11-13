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

The user found the in-game fps counter lacking. A session felt very stuttery during tricks
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
  - `show` (`Last`): the overlay (top left since 2026-10-23: the HUD minimap owns the top right): `FRAME ms / fps` over the last 0.25 s, `1% low`, `0.1% low`,
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
  empty. Reading tool (fork-local): `tools/claude-skills/optimization/tools/frame_log_summary.py`.
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

Problem: the user reported considerable frame-time lag while flipping in the air on large jumps, then
confirmed it happened on every large jump when flipping (session 2026-10-07 13:09, spillway after the observatory). The
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

Measurements so far (headless, 2026-10-08, optimized test build):

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
- **Log survey 2026-10-23 (research b552):** 92 session logs
  (desktop and laptop, none malformed), 12,237 in-play hitch lines: 3.4 % airborne at every size threshold, no
  cluster at a trick start; in the 36 trace-all frames.tsv files about 2,600 flip landings show the same frame times
  in the last 0.5 s of air as on the approach (8.70 / 8.65 ms mean, p99 16.4 / 16.6 ms). So no session so far shows
  an air-specific cost; the Hawaiian Dream report (USER-ASKS 265) has no log yet. The survey found `slowest=` often
  wrong: much of a big hitch sat in no stage, and `post_render` also holds the render / present wait.
- **Change (2026-10-23):** the line adds `map=`, `fixed_other` (fixed-step time outside the three fixed stages, where
  `modding::fixed` runs) and `update_other` (main-thread time outside the stages before `post_render`, so the CPU
  part of `post_render` plus gaps); `slowest=` considers both. `post_render` minus `wait_ms` is its CPU share.
- **Transform propagation spikes (research b554):** in the one
  desktop session with SYSTEM_TIMING (2026-10-20), 42 of 48 hitches of 25 ms or more share their second with a
  single `propagate_parent_transforms` run of at least half the hitch (median 29.5 ms, mean cost 0.36 ms per frame);
  none of 30 laptop logs shows it. Likely Bevy's parallel propagation (workers spin until all finish, the waiting
  thread runs other pool tasks) rather than work [inferred, no trace]. A start-up line `SKATE_THREADS logical=
  compute= async_compute= io=` now records the machine's threads and Bevy's pool sizes (desktop 2026-10-23: 28 / 20 /
  4 / 4). Fixes (a trace first, then serial propagation or fewer workers; `set_if_neq` on per-frame Transform writes):
  todo `transform-propagation-spikes.md`.

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

Evidence (headless, optimized test build, the real scoring runtime and the real HUD movie;
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
changes) from what the mesh already holds, its material only when the color transform bits or the atlas
handle differ, and its `Visibility` only when it differs from the value it last inserted (`Slot::visibility`,
`None` until the first insert so a new slot keeps the spawn default `Inherited` for its first frame, as
before). New slots are spawned exactly as before. Slot order, z, render layer and the panic-free
behavior on a missing texture are unchanged. The main-thread p99 is about 4 us higher (the comparison
runs before a rebuild on frames where a slot changes); traded for removing ~40 render-world re-uploads per
frame on average.

Verification:
- Identity: the old writer is kept verbatim as a test-only reference; the test drives both with the same
  draws every frame and asserts, after every frame, that every slot is identical: entity, `Visibility`,
  `Transform` z bits, `RenderLayers`, mesh and material handle bindings, topology, indices, asset usage,
  every attribute's bytes, color transform bits and atlas texture. Passes on all 2400 frames, twice.
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

Measured on DownTown with `main` (a local script, not kept, validation stubbed): extract 11.5 s (cold
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
   manifest and the DMO (dynamic prop) catalog and write their own folder, so they run on one background thread from the end of
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

- **Every district, every output.** a local script (not kept) converts each of the 10 districts with
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
empty, so an existing installation rebuilds nothing. core, hud, character: unchanged. Customizer fingerprint and
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
- Full fresh setup into a scratch base (a local script, since replaced by `tools/dev-bench/setup_bench.py`, `default.xex`, 3 map workers), `main` vs
  branch, run back to back: total **445.6 → 434.6 s**; map stage 183.1 → 179.8 s (DownTown is the long pole and its
  `prepare` stream decode, cut by #29, dominates); customizer 228 → 234 s (unchanged code; runs after the maps on
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

## Refresh Game Files: unchanged groups are linked, not copied or re-hashed (2026-10-23, USER-ASKS 285)

- **Problem:** a refresh that rebuilds one group (2026-10-23: hud, for the minimap) took about 8 minutes on the desktop
  HDD. The installation is 18.7 GB in 80,929 files (customization 11.7 GB, audio 4.9 GB, maps 1.4 GB).
- **Root cause** (`tools/asset_pipeline/install.py` `_install`, `group_receipts.damaged`):
  1. `damaged()` re-read and SHA-256'd every file of every group not being rebuilt, on every refresh (~7 GB);
  2. the new installation was then built by copying the previous one with `shutil.copy2`; only unchanged maps and
     complete character sets were hard-linked, so audio, stock, living world, park layouts ... were full copies.
  Measured on a bench copy (`tools/dev-bench/refresh_bench.py`, fork-local), cold disk, first run: the audio
  check alone 175 s, the copy 158 s, the hud rebuild 38 s.
- **Change:**
  - A: the audio group's files (4.9 GB), when the refresh does not rebuild audio (just verified by `damaged()`), are
    hard-linked into the new installation (`LINKED_GROUPS` / `unchanged` in `_install`), with the existing copy
    fallback where links fail. First version linked every unchanged group: on the laptop (2026-10-23 15:16, a refresh
    that also rebuilt maps with 8 DLC packs) the maps step rewrote core's `stock/dlc/*-collections.json` and the
    living world's hand props in place, through the links into the published installation; the check below caught
    it (10 files, refresh not published). Only groups that no other step writes are linked now (test
    `test_only_groups_no_other_step_writes_are_linked`); the error lists every changed file. `shared_changed()`
    compares every linked file's size and modification time before publishing; a step that wrote one in place
    fails the refresh, the old installation stays published. `retained._write` (kept DLC side data) now swaps in a
    new file instead of writing in place.
  - B (user's decision 2026-10-23: do B too, skip the rehash when size and mtime match): group receipts record
    `mtime_ns` (`group_receipts.record`); `valid_group_receipt` trusts a file whose size and time both match and
    hashes the rest, adding the time to older entries. Accepted trade-off: a same-size change that keeps the
    modification time is not noticed. The Rust readers of `outputs` use only `size` (`asset_check.rs`, `setup.rs`).
  - B lives in `group_receipts.py`, not `setup_state.py`: `customiser_setup.fingerprint()` hashes `setup_state.py`,
    and a first version there made the customizer build a whole new character generation (+8,294 files).
- **Fingerprints:** all 7 group fingerprints and the customizer fingerprint are equal to the tools before the change,
  so no installation rebuilds anything because of it.
- **Equivalence:** base2 (old tools) and new2 (A + B) from the same bench state: 97,517 files each, all identical
  except the 5 hud outputs, which differ between two old-tools runs as well (the hud export embeds per-run values
  such as its temporary folder `sk8hud-*`); installation.json identical apart from `directory` and `mtime_ns`.
- **Timing (warm cache, the user gaming during part of it, so indicative):** whole refresh 247.6 -> 209.7 s;
  installed-file check 11.6 -> 6.4 s; delete of the old installation 26 -> 13 s. Cold, the check drops from minutes
  (175 s for audio) to seconds, and the copy no longer moves the audio's 4.9 GB.
- **Tests:** `tools/test_setup_shared_links.py` (in-place write caught, replaced / removed files pass, `_write` leaves
  the old copy alone, receipt times: recorded, newer time hashed and caught, same size + time trusted, old entries
  hashed and stamped); setup recovery / customizer / versions tests 26 OK. On the real bench installation
  (`tools/dev-bench/rehash_check.py`): untouched passes without hashing (9.5 s), one byte changed in a 29 MB audio
  file is caught, the original bytes with a new time pass, the file is restored.
- **Launcher (C):** a working-tree version's build stamp now hashes only the exe's inputs (crates, vendor, Cargo.*,
  .cargo, docs/images, maps), so a `tools\` edit no longer rebuilds the game (`tools/steam-launcher/launcher.ps1`).
- **Left:** the copy step is now bound by the number of files (~97k hard links / copies on an HDD, ~95 s warm), not
  by bytes. Avoiding a per-file rebuild of the whole tree would need a different publish design (not done; the
  previous installation must stay intact while the game may still run from it). First-time setup not re-measured.
- **Files:** `tools/asset_pipeline/install.py`, `retained.py`, `group_receipts.py`, `tools/test_setup_shared_links.py`,
  `tools/steam-launcher/launcher.ps1`.


## Map switch: the next map's ambience beds are read while it loads (2026-10-23, USER-ASKS 293)

- **Problem:** desktop session 2026-10-23 16:55 (hails-additions 068db3be): the user felt a stutter at the Challenge
  Map switch University -> DownTown (17:10:54). The log: the commit frame took 498 ms (main 437 ms) while the loading
  overlay was still held, then a 199 ms render wait; the overlay dropped right after (`PC_OVERLAY status: Loaded
  DownTown` at 54.886), and every later frame was normal. So the loading screen froze for about 0.7 s at its end.
- **Root cause (part of it):** the commit frame read the first ambience bed on the game thread (`ambience` 172.6 ms,
  `file_read` 161.1 ms): `ambience::update` asked the read-ahead worker for the map's zone beds only in the new
  map's first frame, and `Library::clip` falls back to a game-thread read for a file not read yet. The rest of that
  frame: `retail_character::load` 115 ms, `living_world::early_load_for_map` 89 ms, `peds::early_load_ped_data`
  84 ms, `vehicles::load_traffic_data` 27 ms, `scoring_hud::reset` 21 ms (SYSTEM_TIMING); the render wait is the new
  map's textures (`prepare_assets<GpuImage>` 295 ms) and meshes (`allocate_and_free_meshes` 197 ms).
- **Change:** `map_audio::read_ahead_next_map` builds the loading map's audio (`MapTransition::loading`, the same
  `build` with its sidecar definition) and queues its zone beds on the worker as soon as the load starts. The bytes
  are the same and the bed starts in the same frame; only when the file is read moves. A map whose `.skate` AUDO tag
  names another district (known only after the load) reads the guess and its own beds as before.
- **Not changed (decided with the user):** holding the overlay longer: both slow frames were already behind it.
- **Verification:** muted run (`tools/dev-bench/frame-time-run.ps1`, University, `SKATE_MAP_SWITCH=25:DownTown`): the
  read-ahead queued 6 beds 4.6 s before the commit (the same 6 the map keeps resident), no ambience file read at
  the commit, the slowest switch frame 282 ms (render wait 222 ms, emitter banks 50 ms). The disk cache was warm
  from the user's session, so the cold-disk gain is not measured yet. Test
  `the_map_being_loaded_is_named_until_it_is_committed`; game_audio / map_transition / frame_timing tests 107 OK.
- **Left:** none; the other commit-frame loaders, the emitter banks and the render upload followed the same day (next
  three sections).
- **Frame-time overlay:** moved to the top left (user 2026-10-23: the HUD minimap owns the top right and the
  overlay covered it), `frame_timing::spawn_overlay`.

## Map switch: game data read on the loader thread (2026-10-24, USER-ASKS 299)

- **Problem:** the map switch's commit frame in the 2026-10-23 16:55 session (previous section) also ran four game-data
  loaders on the game thread: `retail_character::load` 115 ms, `living_world::early_load_for_map` 89 ms,
  `peds::early_load_ped_data` 84 ms, `vehicles::load_traffic_data` 27 ms. All of it was file reads and parsing; only
  the install into the world needs the game thread.
- **Change:** each loader is split into a pure read (`living_world::load_data`, `peds::prepare`,
  `vehicles::read_vehicle_data`, `retail_character::read`) and its install. The map loader thread
  (`map_transition::start`) runs the four reads on their own scoped threads beside the physics and render builds
  (`map_read_ahead::read`) and the commit hands the result over as `PreparedMapData`, keyed by the new map's name and
  generation (lighting by its path). Each loader takes its part only for its own map, once; otherwise (startup, a
  test world, a reader that panicked) it reads the files itself as before. Same functions, same inputs: the installed
  data is the same, only the thread that reads it moves.
- **Moddability:** the ped nav rules are captured when the load starts; when a mod changed them before the commit,
  the read-ahead peds are dropped and the loader reads them with the current rules, so a mod's rules always apply.
  The data stays the files the content layer resolves; nothing new is hard-coded.
- **Tests:** `map_read_ahead::tests` (a part goes only to its map and generation, once; peds only with the same nav
  rules); living_world, map_transition, retail_character and frame_timing tests.
- **Open:** measure the commit frame in a switch run (`tools/dev-bench/frame-time-run.ps1`, `SKATE_MAP_SWITCH`). The
  emitter banks and the render upload: next two sections.

## Map switch: sound banks read during the load (2026-10-24, USER-ASKS 301)

- **Problem:** the switch run of 2026-10-23 (two sections up) still loaded sound banks in the new map's first frames
  (`bank_load` 50 ms): a map change unloads every map bank and forgets the prefetch (`Native::unload_map_banks`), so the
  emitter banks near the spawn and the world (traffic / ped) banks were asked for only once the new map was live and
  then needed at once (`Prefetch::take` found them queued and the game thread loaded them).
- **Change:** the map loader publishes the new map's spawn point as soon as it has read the map
  (`MapTransition::loading_spawn`). `map_audio::read_ahead_next_map` then queues on the prefetch worker, as the next
  map's banks (`Prefetch::request_next`): the emitter banks within `AHEAD` of the spawn, nearest first
  (`emitters::read_ahead_banks`, the same selection `prefetch_near` makes for a listener there), and the world banks
  while the living world publishes (`world_sources::read_ahead_banks`). At that map's change `Native::change_map`
  unloads the old map's banks as before and the queued banks become the requested ones (`Prefetch::change_map`, only
  for the map path they were read for); the emitters mark them requested so they are dropped when left behind. A load
  that fails leaves nothing queued (`drop_next` once the transition is idle); a content change or restart forgets
  them (`Prefetch::clear`).
- **Same outcome:** only the read and decode move. `Runtime::load_bank` still runs in `ensure_bank` at the first use,
  in the same order, with the data `BankSource::load` gives on either thread (test
  `prefetched_banks_are_identical_and_not_decoded_at_the_start`).
- **Moddability:** the banks come from the map's emitter records (disc files, the map's own definition, mods) and the
  install's world bank list, as before; nothing new is hard-coded.
- **Tests:** `prefetch::tests::banks_read_ahead_for_a_map_are_taken_over_only_at_its_change`;
  `emitters::tests::banks_read_ahead_for_the_spawn_are_the_first_frames_banks` (DownTown, data-gated: for spawn
  points across the district the queued banks are exactly the first frame's requests); the emitter, world source and
  installed map transition tests.
- **Open:** measure the commit frame in a switch run (`tools/dev-bench/frame-time-run.ps1`, `SKATE_MAP_SWITCH`).

## Map switch: the render upload (2026-10-24, USER-ASKS 301, 303)

- **Size:** the installed transition test now prints what the render world uploads after a commit: University
  990 MB of textures (RGBA8 arrays with full mip chains, `retail_render::upload_page`) and 153 MB of vertices;
  BlackBoxPark 56 MB / 1 MB (`LIFECYCLE_CHECK ... image_mb mesh_vertex_mb`). The 2026-10-23 switch's render frame
  (`prepare_assets<GpuImage>` 295 ms, `allocate_and_free_meshes` 197 ms) is that copy. The map images and meshes are
  `RENDER_WORLD` only, so extraction moves them (no clone on the main thread) and nothing is copied twice.
- **Change (user 2026-10-24: upload in pieces after the switch, USER-ASKS 303):** the map switch's commit
  (`PreparedScene::publish_sliced`) publishes the materials, buffers and entities at once but keeps the new map's
  meshes and textures in `MapAssets`; `map_render::upload_slices` (PreUpdate, after the transition) inserts them in
  staging order, meshes first, up to 64 MB a frame and at least one asset (`SKATE_MAP_UPLOAD_MB`, 0 = all at once as
  before). The transition stays in its publishing phase (world paused, overlay up) until the last slice is in, then
  runs its three publishing frames as before. Entities whose mesh or texture is not in yet do not draw (their
  material's bind group waits for its images); the user chose this over holding both maps in video memory. The
  startup map still publishes in one pass (behind the splash). Same final world: the same assets under the same
  handles, the same entities; only when each asset goes in changes. Released with the map like every staged asset,
  also when a map is retired mid-upload.
- **Slices:** University 7 frames (one texture array can be larger than the budget and goes in alone), BlackBoxPark 1
  (installed transition test `upload_slices=`).
- **Tests:** `map_render::tests::a_slice_takes_assets_in_order_up_to_the_budget_and_at_least_one`; the installed map
  transition test runs the slices to the end after each commit.
- **Open:** a switch run to measure the render frames per slice (`tools/dev-bench/frame-time-run.ps1`,
  `SKATE_MAP_SWITCH`); the largest single texture array bounds a slice.

## Optimization pass: the 2026-10-24 evening work (USER-ASKS 327)

Skill `optimization-pass` over the loading screen leftovers (doc 31: `sdk.loading` lines and holds, the autosave
corner wheel, FE fade at 60 ticks a second), seated peds reacting to a skater (doc 26f "Seated peds avoid an
approaching skater") and the camera slow-motion subject gate (doc 21 "Camera slow motion").

- **Checklist:** nothing to change. The mod lines are merged once when the loading screen opens (not per frame);
  `LoadingMods::holding` is a set check. The avoid scan runs only for a seated ped on its own phase (every ~1/3 s,
  `(tick + serial) % every`), loops over the few observers and probes the navmesh only for a qualifying approach; the
  clip vectors are built only when an avoid starts. The slow-motion gate does less work than before (no Update
  without the subject). No file I/O, parsing or uploads were added.
- **Desktop runs** (28 threads; `tools/claude-skills/optimization-pass/tools/frame-time-run.ps1`, muted, University
  with a switch to DownTown at 25 s, 70 s, before = 956bb2da, after = 8bd84c24, alternating):

  | run | median ms | p90 ms | p99 ms | max ms |
  |---|---|---|---|---|
  | before 1 / 2 / 3 | 3.13 / 3.13 / 3.14 | 5.62 / 5.63 / 5.64 | 7.06 / 6.86 / 7.40 | 111.8 / 76.5 / 75.8 |
  | after 1 / 2 / 3 | 3.12 / 3.15 / 3.13 | 5.62 / 5.66 / 5.63 | 6.84 / 7.34 / 6.93 | 77.1 / 77.8 / 92.5 |

  No error or panic line in any run; every switch committed. The ped, menu and camera systems never reach a
  `SYSTEM_TIMING` line's top 16 after start-up in either build except `retail_menu_stage::update` (0.005 to 0.009 ms a frame in both).
  The max is the switch frame and varies run to run in both builds. Same outcome: no measurable cost.
- **Laptop** (Ryzen 9 8945HS, 16 threads, Radeon 780M; `second-pc-testing.md` "Engine frame-time test", screensaver
  ended first; before = 2c00e994, after = eb77643a, the same engine change):

  | run | median ms | p90 ms | p99 ms | max ms |
  |---|---|---|---|---|
  | before 1 / 2 / 3 | 4.22 / 4.62 / 4.67 | 7.66 / 7.84 / 7.92 | 8.81 / 9.11 / 9.20 | 118.9 / 117.8 / 115.7 |
  | after 1 / 2 / 3 | 4.83 / 4.58 / 4.73 | 8.01 / 7.84 / 7.89 | 9.34 / 9.00 / 9.16 | 109.4 / 110.4 / 112.5 |

  Within noise: the first before run is the outlier (without it 4.65 vs 4.71 ms). The switch frame was about 7 ms
  shorter after in every pair, likely the faster fade (the loading screen is up a shorter time) or noise; not
  claimed as a gain. Every switch committed, no error lines. Verified the medians from the sent frame logs.

## Build stamp for local builds (2026-10-24, USER-ASKS 328)

- **Problem:** every commit rebuilt the whole game: 2m 27s on the laptop for a docs-only commit, about 2m 10s on the
  desktop. `crates/skate-game/build.rs` watches git's `HEAD`, `index` and `refs` so the build stamp
  (`SKATE_BUILD_ID`, `SKATE_RELEASE_REVISION`: crash reports, the updater) always names the exact commit, and a build
  script that runs again recompiles its whole package. The game is one binary crate, so that is all of it. Even a
  `git add` or a `git status` that refreshes the index started a full rebuild.
- **Change:** `SKATE_BUILD_STAMP=inputs` (opt-in; the Steam launcher's builds set it). The build script then watches
  the build inputs (`crates/`, `vendor/`, `.cargo/`, the Cargo files, the icon) and computes `dirty` over those
  only. While they are clean it does not watch `HEAD` / `index` / `refs`; while they hold uncommitted changes it does,
  so the commit that follows refreshes the stamp as before. Without the variable nothing changes (upstream's exact
  per-commit stamp; release builds run from a clean checkout either way).
- **Trade-off:** after a commit that leaves the inputs alone (docs, tools, notes), the stamp keeps naming the
  previous commit, whose build inputs are the same. The game's code is identical either way.
- **Verification (desktop, `scripts/Build.ps1`):** full build 2m 10s, then an empty commit (stand-in for docs-only)
  0.48 s (cargo `Finished`, 3 s with staging). A source edit built with `dirty=Some(true)` and the old revision; after
  committing it, the next build recompiled and stamped the new revision with `dirty=Some(false)`. The test commits
  were reset.

## Fixed step executor (2026-10-24, USER-ASKS 328)

- **Finding:** the frames that run a fixed step are the slow ones (desktop median about 5 ms against 3 ms;
  laptop 7.6 ms against 4.3 ms, of which the step is 3.8 ms). `SKATE_FIXED_TIMING` on the desktop (University, idle):
  step 2.48 ms = systems busy 1.80 ms + gaps 0.68 ms, cpu 1.84 ms. The systems barely overlap (cpu about equal to
  busy), and the gaps are the multi-threaded executor handing systems between threads plus its sync points. Largest
  systems: `npc_sim::simulate` 0.77 ms, `physics::advance` 0.69 ms per step.
- **Change:** the fixed schedules (`FixedFirst`, `FixedPreUpdate`, `FixedUpdate`, `FixedPostUpdate`, `FixedLast`) run
  on Bevy's single-threaded executor (`app.rs` `single_fixed_executor`); `SKATE_FIXED_EXECUTOR=multi` gives Bevy's
  multi-threaded one. It started as an opt-in with multi as the default; after the desktop and laptop numbers below the
  user chose "single threaded default" (2026-10-25). Players switch in the Steam launcher, Tools > Physics step
  threads (saved in `.local/steam-launcher/fixed-executor.txt`, passed to the game).
- **Same outcome:** the single-threaded executor runs the same systems with the same order constraints and sync
  points; systems with no order between them run in one fixed order, which is one of the orders the multi-threaded
  executor already picks from frame to frame. Systems that use parallel iteration inside still do.
- **What the pauses are (USER-ASKS 330):** a Chrome trace (`--trace <file> --trace-seconds 30`, 20 steps late in the
  capture, `tools/dev-bench/tick_gaps.py`): of a 2.79 ms traced step, 0.365 ms has no system running on any thread.
  No single cause: about 5 us at each hand-off between systems, plus about 20 us for each of the near-empty
  `FixedFirst`, `FixedPostUpdate` and `FixedLast` (Bevy's gizmo and message systems: the multi-threaded executor's
  start-up per schedule run). Trace-all itself does not cost the tick (plain frame log vs trace-all, tick frames:
  `fixed_ms` median 2.399 / 2.393 vs 2.415 / 2.407 ms).
- **Tried and reverted:** `replay::world_live` guards 30 living world systems one by one (`distributive_run_if`, 30
  checks a step); one check per group measured 2.386 vs 2.376 ms (three runs each, plain frame log), within the
  noise, so it stays as it was.
- **Desktop, plain frame log, tick frames' `fixed_ms` median (University, 45 s, alternating):** multi 2.388 / 2.373 /
  2.397 ms; single on `FixedUpdate` only 2.322 / 2.340; single on all fixed schedules 2.228 / 2.243 / 2.212 (-6%
  against multi). Tick frame median 5.00 to 4.85 ms. No errors in any run.
- **Laptop (Ryzen 9 8945HS, Turbo, `SKATE_FIXED_TIMING=1`, e84604a2 = single on `FixedUpdate` only, alternating):**

  | executor | step ms (3 runs) | gaps ms | frame p90 | p99 | max |
  |---|---|---|---|---|---|
  | multi | 3.48 / 3.57 / 3.63 | 0.99 / 1.03 / 1.10 | 7.17 / 7.81 / 8.08 | 8.52 / 9.04 / 9.31 | 79.3 / 37.3 / 53.8 |
  | single | 3.34 / 3.43 / 3.40 | 0.89 / 0.91 / 0.90 | 7.16 / 7.27 / 7.23 | 8.07 / 8.48 / 8.18 | 19.5 / 46.8 / 29.2 |

  Step -4.7%, p99 -8% (8.96 to 8.24 ms mean); every single run beat every multi run. The schedules added since
  (all fixed schedules) are not measured on the laptop yet.

- **Overlay row and launcher page (2026-10-25, USER-ASKS 336 / 337):** Physics Step Threads is a row of the overlay's
  Opt-Ins section, saved as `physics_threads_multi` in `settings/graphics.json` and read when the app is built
  (`graphics_menu::physics_threads_multi`; `SKATE_FIXED_EXECUTOR=multi|single` still overrides it for a run). The
  launcher's Tools > Opt-ins and settings page writes the same field instead of its own `fixed-executor.txt`, and
  passes it on as `SKATE_FIXED_EXECUTOR` for older game versions. Applies at the next start.

## Trace-all mode (`SKATE_TRACE_ALL=1`)

Status: done, in #52; used in play sessions since 2026-10-17.

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

Status: done (2026-10-08), committed 8f126813.

Problem: after a trace-all session (University, 2026-10-08 09:58) the user reported very poor fps and lag in the trace-all mode. The perf report showed every window at a 33.6 ms median (p95 about 34.6 ms) with the main schedule at about 9 ms, physics about 3 ms, render CPU about 3 ms and the GPU passes about 9.5 ms in total. The suspect was the per-pass GPU timestamp and pipeline statistics queries that `SKATE_PERF_GPU` adds (Bevy `RenderDiagnosticsPlugin`) on every pass of every frame.

Root cause of the 33.6 ms: not the queries. The settings file every launcher version shares (`data/installations/<id>/settings/graphics.json`, written 2026-10-07 23:20) has `"fps": 30`, the graphics menu's FPS limit, and `graphics_menu::pace` sleeps every frame up to 1/30 s. The game runs with `PresentMode::AutoNoVsync`, so nothing else locks the frame rate. The session's frame log agrees: frame median 33.56 ms against a main thread median of 8.76 ms, and `AUDIO_TIMING frame=33.6 ms`. The missing ~24 ms per frame is the limiter's sleep. Turning Esc > GRAPHICS > FPS limit back to Off brings the frame rate back.

The queries still cost something, so they are sampled now. Bevy's readback is asynchronous (`map_async`, collected on a later frame), so the queries never stall the CPU; their cost is GPU work and resolve copies on every pass.

Change (`crates/skate-game/src/performance.rs`):
- `SKATE_PERF_GPU_EVERY=<frames>`: how often the GPU queries run. Default 30 in trace-all, 1 (every frame, the old behavior) for the one-shot `SKATE_PERF_REPORT` benchmark; 0 turns the GPU queries off while every other trace stays on. A launcher version or user can set it like any other switch (`versions.json` `env`). The value is logged at startup (`SKATE_PERF_GPU sample_every=`) and written into the report (`gpu_sample_every_frames`).
- Bevy's render system only records GPU queries while its diagnostics recorder resource is in the render world (it removes it, runs the graph, puts it back). A render world system right before `RenderSystems::Render` moves the recorder aside on frames that are not sampled and back on sampled ones. The recorder type is crate-private in bevy_render, so it is named through the public `RenderContext::new` signature; no vendoring.
- Bevy smooths render diagnostics over about 0.1 s, so with every frame sampled the report already held roughly the newest frames; with one frame in 30 it holds the newest sampled frame. Readbacks in flight are collected on the next sampled frame, so values arrive up to one interval late.
- Nothing else changes: frame log, audio state log, perf windows, render phase split and log lines are written by the same code as before.

Measured (release build, same build and scene for every mode: University spawn, idle, muted, windowed 1280x800, FPS limit off through a separate settings folder, RTX 4080 SUPER, Vulkan; 60 s per run, first 25 s dropped; two rounds, the second in reverse order; tool `tools/dev-bench/traceall-gpu-bench.ps1`):

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

## Trace-all from the Steam launcher, setup's own packages (2026-10-17)

Problem: trace-all (above) was only reachable from the main PC's private launcher copy; a second PC running the
committed `tools/steam-launcher` had no way to record a full session. On a source checkout, the game's
**Refresh Game Files** stopped in the audio group with "No module named 'numpy'": the game starts `tools/setup.py`
with a system `py -3.13`, and the converter's packages were only installed by hand.

Change:
- `tools/steam-launcher/launcher.ps1`: entry `rust-traceall` ("Rust engine: play + trace all"): `SKATE_TRACE_ALL=1`
  with every output in the version's `logs\trace-all-<time>\` (`frames.tsv`, `audio_state.tsv`, `perf.json`; the
  engine writes the rest next to them). Versions without `SKATE_TRACE_ALL` get the single switches they have; versions
  without any are hidden. Its result screen counts malformed rows in `audio_state.tsv` and `frames.tsv` (skipping
  `frames.tsv`'s leading `#` comment line).
- Launcher: optional `after_session` send script (run with `-SendOnly` after each session, and at the next start for
  a session closed with Steam's **Exit game**; `launcher.ps1 send`). The next-start `pending` check now also covers
  trace-all and plain engine sessions. A version without converted game files is not started; the result screen says
  to run `tools\setup-dev-assets.ps1`.
- `tools/setup.py`: `ensure_dependencies()`: on a source checkout without numpy / Pillow it creates or updates
  `.local\venv-setup` from `tools/requirements-setup.txt` (without pyinstaller) and runs setup again in it; a failed
  install shows a message. The packaged setup exe is unchanged. "Could not reuse the previous Xbox source" only shows
  when the source failed, not for every error.
- `tools/setup-dev-assets.ps1`: headless setup for a source checkout (the same venv, `install()`, the `assets`
  junction, `--check-assets`), for checkouts without the setup window.

Verification: `py -3.13 -m unittest tools.test_setup_dependencies tools.test_setup_dead_console tools.test_setup_refresh`
(10 tests); a bare Python venv ran `tools/setup.py --help`, installed numpy 2.2.6 / Pillow 11.3.0 into
`.local\venv-setup` and re-ran; on a laptop (source checkout): a `rust-traceall` session gave `audio_state.tsv` 2328
rows and `frames.tsv` 28614 rows, 0 malformed; `setup-dev-assets.ps1` converted and linked the files (its final check
was fixed for Windows PowerShell 5.1 afterwards). `launcher.ps1 entries` lists `rust-traceall` for a checkout with
trace-all.

Open: when the game restarts itself into setup (Refresh Game Files), the launcher's session record and send did not
happen once on the laptop; not reproduced yet.

## Steam launcher: the game comes to the front, the console gets out of the way (2026-10-17)

Problem (laptop, user): the launcher prompt window often stays over the game, and neither the game nor the
prompt window can be clicked to move them. The launcher console is maximized and in the foreground (`Focus()` /
`Focus-Console`, `ShowWindow` 3) right before the launch; the game starts as a child of that console
(`Run-Bat` -> `cmd /c PLAY.bat`, no window of its own), so Windows' foreground lock could open it behind the console.
A maximized window cannot be dragged, and a click into the console with QuickEdit on starts a text selection that
freezes its screen until Esc.

Change (`tools/steam-launcher`): `launcher.ps1` `Run-Bat` starts the bat without waiting, then `Hand-Over-To-Game`
waits (up to 180 s) for a `skate3rust` / `skate3` window, gives it the foreground while the console still holds it and
minimizes the console without activating it (`ShowWindow` 7); the result screen restores the console as before.
`SkateLauncher.cs` turns QuickEdit off for its console (`SetConsoleMode` without `0x40`). The menu stays maximized.

Verification: `csc` builds `SkateLauncher.exe`, `launcher.ps1` parses (PowerShell 5.1). Laptop session 2026-10-17
13:34: the game came to the front, but the launcher stayed minimized after it.

Follow-up (same day): on Windows 11 with the default terminal "Let Windows decide", Steam's launch opens the
launcher in Windows Terminal. Its pseudo console window (`GetConsoleWindow()`) takes a minimize but not the restore /
maximize / foreground the result screen needs, and the console font call has no effect there. `SkateLauncher.cs`
`InClassicConsole`: inside Terminal (`WT_SESSION` set) it starts itself again under `conhost.exe` (a classic console
window, marker argument `--conhost`), minimizes the Terminal window and waits, returning the child's exit code, so
Steam still tracks the session. Desktop test inside Terminal (`wt -w new SkateLauncher.exe notes`): the classic
"Skate launcher" window maximized, the Terminal window minimized, both closed with the launcher, no processes left.
Not checked on the laptop yet: the result screen coming back after a game.

## Steam launcher: a real window, Refresh Game Files, first-time setup (2026-10-18)

Problem (laptop notes 2026-10-18, user): the Steam launcher needs to become a real launcher, since the
terminal window still does not work properly, and it needs a Refresh Game Files option, so the game need not be
launched to run it. Asked the same morning: entries grayed out until their config is set up; a hails-additions
version on the fork and one for upstream main, always there and rebuilt when newer; a first-time setup that installs
the dependencies; a size cap for the build folders that clears old build caches before a build; and Tools options to
clear all builds, one build or one build cache.

Found on the way (laptop session `game-20261018-094840`): the in-game Refresh Game Files restarts the game into setup
(`setup::restart_with_refresh` spawns a new `skate3rust.exe --refresh-assets`), and the old process exits. The
launcher took that exit for the end of the session, so it checked and sent the logs at 09:49:41 while setup was still
converting (the setup ran until about 09:55 and ended with exit code 1; its output is in the laptop's local log, after
the part that was sent). `launcher.ps1` `Wait-GameTree` now waits until no game process runs before the check and
the send.

Change, engine (`setup.rs`, `main.rs`): `skate3rust.exe --update-game-files [--assets <dir>]`, a windowless tool mode
before the crash supervisor and the splash. It runs `asset_check::check` (the start-of-game check, synchronously) and,
when it finds a problem, the same refresh as Extras > Refresh Game Files: the out-of-date groups are marked stale and
their converter versions cleared in `installation.json` (`clear_stale_pipelines`, factored out of
`restart_with_refresh`), then setup runs (`refresh_dev_copy` for a development copy, `run_release_setup` for a
release copy; the latter is also what `asset_root` now calls). Log lines `ASSET_CHECK ...`, `GAME_FILES current` /
`refreshed` / `failed: <why>`; exit code 0 = current or refreshed, 1 = the refresh failed, 2 = no game files yet.

Change, launcher (`tools/steam-launcher`):
- `SkateLauncher.cs` is a WinForms window (`/target:winexe`), full screen and owner-drawn (no focusable buttons:
  Space is Steam's local "B"). Input: XInput and the keyboard (arrows, Enter, Esc / Space / Backspace), because Steam
  gives a launcher window its desktop layout (keys) and the game its gamepad layout once the game is in front
  (Steam's own logs on this PC: the console launcher got the desktop configuration, the shortcut's layout came about
  5 s later with the game window). A press that arrives from both sources within 150 ms counts once. Input is read
  only while the window is active and no job runs (XInput reports the pad to every process). Jobs run `launcher.ps1`
  hidden, with the output shown live; `AllowSetForegroundWindow` before the start, the window hides once a game window
  exists and comes back with the result.
- `launcher.ps1 entries` lists every entry with a group, on/off and why it is off; the window draws off entries gray
  with the reason (recomp without `recomp.exe`, RPCS3 without `rpcs3`, send without `after_session`, the recomp share
  tools without `tools\second-pc` set up, traces a version's source doesn't have, Refresh Game Files on a version built
  before `--update-game-files`).
- Built-in versions `hails-additions` (fork) and `upstream-main` (SK8-ENGINE), detached launcher-only worktrees that
  fetch their branch from a URL (`fork_url`, `upstream_url`) on every launch and rebuild when it moved; a
  `versions.json` entry with the same id replaces one. Merged in from the main PC's private copy: `detached`, the
  full-refspec fetch (the fork's rolling release tag has the branch's name), re-pointing a version's assets link after a
  setup refresh, the "NOT UPDATED: local changes" warning, the working-tree hash for `dev` versions, per-version `env`,
  RPCS3 entries.
- Tools: Refresh Game Files (`tool-refresh`), build now, send now, the recomp's newest build and save
  (`play-recomp.ps1 -Update` / `-GetSave`), first-time setup, Clear builds and build caches (`clear-cache`,
  `clear-build`, per version or `all`, each with its size and a confirmation page).
- `setup.ps1`: checks Git, the C++ build tools (MSVC, Windows SDK `rc.exe`, CMake, via vswhere), LLVM
  (`llvm-readobj`, `scripts/Build.ps1`), Python 3.13, Rust (`rustup show active-toolchain`), the GitHub CLI (optional)
  and Vulkan; `-Install` runs the machine-wide winget installs one after another in ONE elevated step (one UAC prompt;
  two installers at once broke the laptop's Rust install, `second-pc-testing.md`), plus `LongPathsEnabled`, then
  rustup unelevated (per user). The window opens on this page while a required tool is missing.
- Size cap: `build_cap_gb` (and optional `build_cap_dir`, `build_reserve_gb`, `target_root`) in `config.json`.
  `Make-Room` runs before every build: while the build folders (or `build_cap_dir`) plus the reserve are over the cap,
  it deletes the build cache of the version built longest ago (by its exe's time), never the one about to build, and
  only the launcher's own version target folders.

Verification (main PC, 2026-10-18): `cargo test -p skate-game --bin skate3rust -- setup::` 8 passed (new
`update_game_files_refreshes_only_what_the_check_finds`: current files run no refresh, each out-of-date group once, a
failed refresh is reported). `skate3rust.exe --update-game-files --assets <this PC's assets>`: `ASSET_CHECK done in
821 ms: 0 problem(s)`, `GAME_FILES current`, exit 0, no window; a missing folder: exit 2. `launcher.ps1 entries /
versions / status / sizes`, `setup.ps1` (all found here). The window: main menu, version picker (the built-ins
listed), grayed Recomp / RPCS3 rows with the reason, a one-click `tool-setup` run that showed "everything is
installed" and closed after its countdown. Size cap on a scratch checkout with three 3 MB build folders and a cap of
8.5 MB: the oldest folder went, the one to build and the newer one stayed. `launcher.ps1 tool-refresh` end to end
(build of this checkout into its K: target, then the check): "Refresh game files: current: nothing to refresh". The
Clear page listed this checkout's 79.7 GB build cache and 83.2 GB whole build; its confirmation defaults to "No, keep
it". Self-update: `pending` (run at every start, also by the old console exe) rebuilds `SkateLauncher.exe` when
`SkateLauncher.cs` is newer, renaming the running exe aside (tested: new exe in place, the old one deleted at the next
start), so a laptop that pulls the new launcher gets the window without running `build.bat`. Not tested: an actual refresh through
`--update-game-files` (no out-of-date files here), the elevated installs (everything is installed on this PC), the
window under Steam / Steam Link (laptop).

Follow-up (laptop session 2026-10-18 12:06, launcher version hails-additions): a Refresh Game Files in one copy
published a new installation and deleted the old one; only the link of the copy that ran it was re-pointed, so the
main checkout and upstream-main failed at start ("Asset root assets: The system cannot find the path specified").
`launcher.ps1` `Repair-AssetLink` / `Repair-AllAssetLinks`: every checkout's `assets` junction that no longer resolves
is pointed at the current installation (`data\installation.json`) at every start (`pending`), before a version runs
and after every session; a real folder is never touched. Tested on a scratch checkout with both links on a deleted
installation: both repaired. The send after a session missed a version's logs (they are in its own `logs\`):
`tools/second-pc/play-and-send.ps1` now also collects every `.local\versions\<id>` checkout's `logs\` and `bin\logs\`
and the one the launcher names in `SKATE_SEND_LOGS_DIR`. And the restarted refresh logs to its own
`logs\game-<yyyyMMdd-HHmmss>.stderr.log` (`setup::refresh_log`; `game-refresh-<unix seconds>` until 2026-10-18): the copy that restarts exits, the launcher closes
its session log, and the two refreshes of the day ended with exit code 1 and no line saying why.

## Steam launcher: A / Enter did nothing on the main page, no mouse (2026-10-18)

Problem (laptop note launcher-no-select, user): in the launcher neither A nor B selects
anything and the options cannot be clicked, though it reads the d-pad. The laptop's own XInput poll
read A / B / D-pad on pad 0 with the launcher in front, so the pad was fine.

Root cause: in `Act()` the main page's grid branch (`if (IsMain(p)) p.Sel = GridMove(...)`) came before the "ok"
branch, so every action on the main page went to the grid move, which ignores "ok": A and Enter did nothing there
(the D-pad worked because the grid move handles directions; B on the main page re-opens the main page by design).
The window had no mouse handling at all.

Change (`tools/steam-launcher/SkateLauncher.cs`): "ok" is handled before the grid move; the last paint records each
tile's / row's rectangle, hovering selects it, a left click runs it, a right click goes back.

Verification: compiles with the framework csc (exit 0). Not tested on the laptop yet.

## The world sound banks decode behind the loading screen (2026-10-18)

Problem: a DownTown start from a cold disk froze the game for 4028 ms about 3 s into play; `SYSTEM_TIMING` put 3.8 s of
it in `game_audio::world_sources::frame`, at "AUDIO_WORLD on". The world banks (traffic and peds) have a prefetch
worker, but the native runtime (`Native`, which owns the worker) exists only once the audio's first start is done,
right before the loading screen goes, so `frame` asked for the banks then; the first owners spawned at the next
fixed tick, and `Prefetch::take` on a bank still queued returns nothing, so the game thread decoded nearly all of
them itself (the worker decodes one bank at a time). Warm, the same loads took about 0.1 s, so it shows on a first
start after a reboot.

Change (`world_sources.rs`): a startup hold `world_audio_banks` ("Loading the world sounds"), taken at Startup when
the world audio and its prefetch are on, released by `release_banks_hold` once the `audio` and `living_world` holds
are gone and either no runtime started, the living world will not publish on the map, or every world bank is loaded,
missing from the install, or requested and no longer queued or decoding (`banks_settled`, `Prefetch::busy`); 30 s
cap. Only the waiting moves: the banks still `load_bank` at the first owner, from the worker's data, as before.

Verification: warm DownTown runs: `AUDIO_WORLD banks ready behind the loading screen in 85 ms`, the loading screen
4.5 to 5.5 s as before. Tests `the_world_banks_hold_waits_for_the_audio_and_the_living_world` and, data-gated,
`world_banks_are_prefetched_when_expected_and_load_without_decoding` (now also asserting `banks_settled` once
decoded). Not reproduced cold (the file cache could not be dropped without admin rights).

## The mods' ground has no mass (2026-10-18)

Problem: the first fixed tick of play took 88 to 97 ms on every DownTown start. A trace from launch (`--trace`, new
spans `mods.ensure_ground`, `mods.triggers`, `mods.player_shapes`, `mods.sync`) put 51 ms in `modding::fixed`:
`mods.ensure_ground` 26 ms (attaching the prebuilt ground collider) and `mods.dynamics_step` 26 ms (its first step).
Rapier computes a collider's mass properties from its shape when it is attached to a body and again at the next step;
for the city trimesh (reproduced with 1.5 M triangles: 25 ms and 26 ms) that is a pass over every triangle, for a
fixed body whose mass rapier never uses.

Change (`skate-dynamics`): `build_ground` builds the ground collider with density 0 (`build_ground_with_density`),
so rapier skips the mass computation. Same simulation: `a_massless_ground_simulates_like_the_default_one` drops a box,
a sphere and a capsule onto a hilly ground built both ways and compares every body snapshot and contact of 240 steps
as text (bit for bit). `a_city_ground_installs_and_takes_its_first_step_quickly` (ignored, timing): install plus first
step of the 1.5 M triangle ground 16.7 us (was about 50 ms). DownTown: the first fixed tick 97 ms -> 39 ms; what is
left is mostly the DMO census's first fill (`dmo_stream`, 23 to 29 ms, the prop collision layer rebaked once).

## No mods snapshot without a reader (2026-10-18)

Problem: with no mod installed, `modding::update` still cost 0.63 ms of main thread a frame in DownTown (2.2 to 2.4 ms
on the laptop) and `mods.snapshot` ran 348 times in 9 s. The new `MODS_BUSY <reason>` line (logged when the reason
changes) showed why: "skater proxies", the living world's NPC skaters, peds and cars mirrored into the mods' world so
mod bodies can hit them. They kept the runtime busy, so `fixed` and `update` built the full JSON snapshot every tick
and frame with no mod to read it.

Change (`modding/mod.rs`): `snapshot_has_reader` = busy for any reason but the proxies (a running mod, commands,
bodies, graphics, overlays, joints, an attach, canvases, volumes). `fixed` builds the snapshot only then; `update`
returns early without one unless there is a reader, the overlay is open (the mods menu can start a mod there), or the
next folder scan is due within two frames (`Manager::scan_due_within`, `maintenance`'s scan can start a mod), so a
starting mod still gets the snapshot of the frame before, as before. The proxies themselves still step and sync.
Limit: a frame more than twice as long as the one before, right at a scan that starts a newly dropped-in mod, would
hand it a snapshot one frame older than before.

Verification: test `skater_proxies_alone_need_no_snapshot`. DownTown, no mods, 60 s, warm: median 3.30 -> 3.23 ms,
p90 5.69 -> 4.90 ms, p99 7.28 -> 6.12 ms, `FRAME_HITCH` lines 60 -> 43; `modding::update` and `modding::fixed` left the
top 16 of `SYSTEM_TIMING`. Workspace tests: 4211 passed, the Skyline example test the only failure (needs a local
asset).

## Map switch: the mods' ground built by the map loader (2026-10-17)

Problem: every map switch froze the game for 1 to 2 s on the first frame on the new map (laptop session
2026-10-17: 2132.6 ms on a Ryzen 9 8945HS, University to DownTown; desktop: 1037.6 to 1313.3 ms). `FRAME_HITCH`
billed it to the "audio" or "pre_update" stage, depending on where the frame's systems fell between the stage markers,
with none of the measured audio parts near it.

Root cause [trace]: a Chrome trace of the switch (`SKATE_MAP_SWITCH`, below) shows `mods.fixed` taking 1138 ms on the
first fixed tick on the new map, and 755 ms on the first tick after start. `modding::ensure_ground` rebuilt the mods'
dynamics ground (`skate_dynamics::DynamicsWorld::set_ground`: one Rapier trimesh of every world collision triangle,
with fixed internal edges) on the game thread, after the map change cleared it. It runs whether or not a mod uses it.

Change:
- `skate-dynamics`: `set_ground` split into `build_ground` (the collider, built apart from any world, so on any thread)
  and `install_ground`; `set_ground` is now the two in a row.
- `map_transition`: the map loader builds the mods' ground from the new map's physics on its own `mod-ground` thread
  (next to the skater and camera loads) and hands it over at commit as `modding::PreparedGround`, tagged with the new
  map generation.
- `modding::ensure_ground`: on its first tick on a map it installs the prepared ground of that generation; with none
  (start, a mod reload on the same map, a failed build) it builds it as before. The ground is ready on the same tick,
  from the same triangles, so mods see the same world.
- Diagnostics: `SKATE_MAP_SWITCH=<seconds>:<map .skate path>` requests one map switch that long after start, for
  unattended measurement (`tools/dev-bench/map-switch-run.ps1` on the main PC).

Verification: `cargo test -p skate-dynamics a_ground_built_apart_is_the_same_as_set_ground` (raycast hits and a settled
crate identical, built on another thread); `cargo test -p skate-game scripted_switch`; map switch University to
DownTown on the main PC, muted, before / after: the first frame after the commit went from 1313.3 ms (1294.8 ms fixed) to
254.1 ms (235.4 ms fixed); in a trace `mods.fixed` on that tick went from 1138 ms to 51 ms, and `build_mod_ground`
(1414 ms) ran on its own thread; map loading (behind the loading screen) went from 3869 to 4191 ms. Left in that
frame: the living world loading its map data on the first fixed tick (`peds::load_ped_data` 141 ms,
`living_world::load_for_map` 35 ms), a follow-up.

Not changed: the first ground at start (still built on the first fixed tick). Correction 2026-10-17: that tick is
not behind the loading screen; virtual time is paused there, so it is the first frame of play (1.3 s with the
checkout's dev mods on the desktop, `modding::fixed`; see "The living world loads behind the loading screen"). Built behind the loading screen since 2026-10-18 (section "The first ground at start is built behind the
loading screen").

## Map switch: the ped data parses `tables.json` once (2026-10-17)

Problem: after a map switch the first fixed tick loads the living world's ped data (`peds::load_ped_data`), 150.3 ms
on the desktop (University -> DownTown, `tools/dev-bench/map-switch-run.ps1`); on the laptop the same frame is longer.
New log line `PED_DATA_LOAD <map>: total (bodies, nav, graphs, tables, plugins, hand props)` showed where: the
1.8 MB `private/living_world/tables.json` was read and parsed eleven times (the body tables, eight mood / chase /
takedown / sight / conversation / sit / hand prop readers, the plugins and the hand props), about 7 ms each.

Change: `load_ped_data` parses it once and hands the parsed tree to every reader (`ped_mood::*_in`,
`PedData::load_with`, `PluginData::load_with`, `HandPropData::load_with`). A reader given none reads the file itself, so
a missing or broken file reports the same errors as before. The byte-taking readers stay for the tests.

Proof of identical behavior: the same parsed `serde_json::Value` goes through the same reader code; the loaders'
summary lines after the switch (`PED_GRAPH`, `PED_MOOD tables`, `PED_PLUGINS`, `LIVING_WORLD peds:`) are the same as in
the session log of 2026-10-16 23:46, except `PED_HAND_PROPS ... with models` (0 -> 22), which comes from
`hand_props.json` of the refreshed install (this change does not touch that read). skate-game ped tests 71 pass
(15 ignored, as before), including the data-gated loads from the export.

Result: `PED_DATA_LOAD DownTown` 150.3 -> 79.2 ms (bodies 33.0 -> 34.8 incl. the one parse, tables 53.8 -> 2.3,
plugins 32.1 -> 14.8, hand props 7.9 -> 0.5; nav 22.5 -> 25.8 and graphs 1.0 -> 1.1 unchanged code, run-to-run noise).
Left: the body clips and the per-map navmesh (about 60 ms), `load_for_map` (about 35 ms).


## Low-end PCs: the GPU and graphics settings in the log (2026-10-17)

Problem: a laptop session felt laggy at 100-150 fps (local todo low-end-pc-performance). Its log named the GPU
(`AdapterInfo ... DiscreteGpu`) but not the framerate cap or the other graphics settings the run started with, and a
hybrid laptop that Windows puts on the integrated GPU shows no sign of it beyond being slow.

Change:
- `graphics_menu::setup` logs `GRAPHICS_SETTINGS <w>x<h> scale <n>% framerate cap <n|unlimited> npc draw distance <x>
  frame stats <bool>` once at start (changes in the menu already log as `PC_OVERLAY now ...`).
- `app::warn_integrated_gpu` (Startup): only when the picked adapter is integrated, it lists the Vulkan adapters and
  warns `GPU_ADAPTER integrated <name> picked although a discrete GPU is present (<names>): set skate3rust.exe to
  "High performance" in Windows Settings > System > Display > Graphics`; without a discrete GPU an info line.
  Nothing runs on a discrete adapter.

Also found while measuring (not changed): the frame after a map switch commits uploads the new map's textures and
meshes in one go on the render thread (`prepare_assets<GpuImage>` 266 ms, `allocate_and_free_meshes` 145 ms on the
desktop); it falls in the switch's three "publishing" frames behind the pause menu, so play is not affected. Bevy's
`RenderAssetBytesPerFrame` could spread it, at the cost of the map appearing piece by piece.

Verification: skate-game graphics tests 18 pass; a muted desktop run logs `GRAPHICS_SETTINGS 1280x800 scale 100% framerate
cap unlimited npc draw distance 1.00 frame stats true` and no `GPU_ADAPTER` line (RTX 4080 SUPER, discrete). The integrated GPU
path needs a hybrid machine set to the integrated GPU (not checked).

## Per-system cost readout (`SKATE_SYSTEM_TIMING`, 2026-10-17)

Problem: the frame log gives the frame time and the trace-all Chrome trace gives spans, but neither says which Bevy
systems cost the most per frame without loading a trace into a viewer, and a laptop session only sends logs.

Change (`crates/skate-game/src/system_timing.rs`, plan from research b214 widened to every system):
- `SKATE_SYSTEM_TIMING=1`, also on under trace-all, installs a tracing layer that names each Bevy `system` span once
  when it is created and adds up the wall time between enter and exit per system, with no allocation per run.
- A system in `Last` logs once per second `SYSTEM_TIMING frames=<n> systems_ms_per_frame=<x> | <name>=<mean ms per
  frame>/<longest run ms>x<runs> ...` for the top 16 (name = last two path segments), then resets the counters.
- Parallel systems overlap, so the numbers add up to more than the frame; they show where the CPU goes. Off: the layer
  is not installed and nothing runs. No game state is read or changed.

Verification: a muted 60 s DownTown run without mods (`frame-time-run.ps1 -NoMods`, which now keeps the lines in the
run's trace folder as `system_timing.log`) logs one line per second; its top leaf system pointed at the car material
fix below.

Added 2026-10-18: after the top list, `| slow name=<ms> ...` lists every system with one run of at least 10 ms,
whatever its rank (one long wait is small per second and stays out of the top list). Diagnostics for the stalls
below: `SKATE_PRESENT_MODE=fifo|mailbox|immediate` and `SKATE_FRAME_LATENCY=1..3` override the swapchain
(default AutoNoVsync and wgpu's latency; `SWAPCHAIN_OVERRIDE` log line when set).

## Car materials are no longer uploaded again every frame (2026-10-17)

Problem: the first `SYSTEM_TIMING` run showed `prepare_erased_assets<VehicleMaterial>` at 0.32 ms per frame and
`prepare_material_bind_groups` at 0.27 ms, every frame, although no car's look changed.

Root cause: `retail_character::update` writes the eased `display_sh` into `Lighting` every frame, so `Lighting` is
always "changed". `vehicles::update_car_light` took that as a new light and called `get_mut` on every car material;
`get_mut` alone marks the asset modified, so Bevy prepared and uploaded every car material again each frame with the
same values.

Change: `update_car_light` writes only a material whose `params.light` differs from `Lighting::light`. The values the
GPU sees are the same as before on every frame (a skipped write is one whose value already matches), so the picture
is unchanged; only the redundant uploads go. Test `car_light_writes_only_materials_whose_light_differs` (an unchanged
light leaves the material asset untouched, a new light still reaches it).

Verification (desktop, DownTown, muted, no mods, 60 s, before / after on the same build otherwise): median frame
3.48 / 2.68 ms, p90 4.82 / 4.64, p99 6.81 / 5.94; frames in the run 13800 / 16664; summed system time 11.3 / 9.9 ms
per frame; both car material systems leave the top list; 0 error lines in both runs.

Open (found with the same readout, not fixed): steady-state stalls of 25-90 ms, about one every 1-2 s, also in a
foreground desktop play session (144 in 4 minutes, mean 31 ms). Their lengths cluster near 33-36 ms and 66-69 ms. In
every second with a stall the same systems on different threads stretch together by the same amount
(`propagate_parent_transforms` and the specialization checks on the main thread, `render_system` on the render
thread), so the whole process waits on something shared rather than one system being slow. Ruled out: the frame rate
(a 144 fps cap keeps them), the NPC physics opt-in (fewer with it off, not gone) and log writing (already on its own
thread). A Chrome trace hides them because it slows every frame; a Windows Performance Recorder trace of one stall is
next. 2026-10-18: the stretching systems all wait inside ComputeTaskPool scopes, and a waiting scope runs unrelated
pool tasks meanwhile (bevy_tasks `task_pool.rs` scope, tick on), so one blocked task stretches whichever system
picked it up, on the main or the render thread (research b230). Not the swapchain: `prepare_windows` never shows in
the `slow` list, and Fifo / Mailbox / Immediate / frame latency 1 all keep them (10 to 38 frames over 25 ms per 60 s
against 24 and 37 for the default, run-to-run noise).

## The living world loads behind the loading screen (2026-10-17)

Problem: the living world's map data (`load_for_map`, about 35 ms on the desktop) and the ped data
(`load_ped_data`, about 79 ms) are FixedUpdate systems. Virtual time is paused while the startup loading screen is
up and while a map switch publishes, so no fixed tick runs there: both loads landed in the first frame of play
(research b226; verified `startup_loading.rs` and `map_transition.rs`).

Change (plan b228, checked and extended):
- The same loads run in PreUpdate after the map transition while the loading screen is up
  (`living_world::behind_loading_gate`: startup not done, or a map transition busy). Both are keyed by
  (map, generation), so the fixed copies on the first tick find the map loaded and do nothing.
- Startup takes a `living_world` hold on the loading screen and releases it once both are loaded. At a switch the
  load runs in the commit frame, three Publishing frames before the unpause.
- The first tick stays the same: the late load ran the crosswalk upkeep on the old data and then replaced it, so a
  map's first tick had no signaled arms; an early load skips that upkeep once (`PedData::skip_arms_once`).
- Settings read only at load (the seed, `enabled`, `pedestrians.enabled`) that change before play starts make the
  first tick load again with the current ones (`recheck_early_load`), as the late load would have.
- Added 2026-10-18: the traffic data (`vehicles::load_traffic_data`, about 22 ms; it parses `tables.json` and
  `vehicles.json`) joins the early loads the same way. At startup nothing changes (no cars yet); at a switch the
  old map's cars go during the loading frames, so the first tick no longer pushes them as collision proxies into
  the new map (`push_vehicle_proxies` runs before Physics). Its RNG is not touched by the load. A map switch run
  logs `LIVING_WORLD traffic models` once per map, at startup before `STARTUP_LOADING done` and right after the
  commit.
- Ordering made explicit: the NPC skater chain (`avoid`, `simulate`, which read the ped navmesh) runs after
  `load_ped_data`. Before, only Bevy's shared automatic sync point after the two record systems' Commands ordered
  them; same order, now stated.
- Same outcome, earlier: mods' `living_world` despawn events for the old world reach their Update callbacks during
  the loading frames instead of the first play frame (same events, same order; the fixed readers still get them on
  the first tick, Bevy 0.18.1 keeps messages until a fixed tick ran). The world audio banks' prefetch now starts
  behind the loading screen, so the first seconds of world audio can start sooner.

Tests: `the_loading_screen_loads_the_living_world_and_the_first_tick_keeps_it`,
`settings_changed_behind_the_loading_screen_load_again_on_the_first_tick`,
`npc_skater_systems_are_ordered_after_the_ped_data_load` (built without automatic sync points, it names `avoid`,
`advance` and `simulate` when the order is missing).

Verification: skate-game tests 788 pass. A muted map switch run (`map-switch-run.ps1`, University then DownTown,
dev mods): `LIVING_WORLD data` and `PED_DATA_LOAD` come from `early_load_for_map` / `early_load_ped_data`, at
startup before `STARTUP_LOADING done` and at the switch right after `MAP_TRANSITION_COMMITTED`; each logs once
per map (no second load on the first tick, no settings reload); 0 error lines. The first frame with a fixed tick
after the switch took 6.1 ms (fixed step 1.6 ms), where that tick carried the whole load before (about 115 ms
on the desktop). At startup the first fixed tick still took 1.3 s with dev mods: the mods' first ground
(`modding::fixed`), moved next ("The first ground at start is built behind the loading screen").

## The first ground at start is built behind the loading screen (2026-10-18)

Problem: the mods' dynamics ground (one Rapier trimesh of every world collision triangle, built whether or not a mod
uses it) was built on the game thread on the first fixed tick at start, which is the first frame of play: frame 6 of
a muted DownTown run without mods took 1718 ms, 1714 ms of it fixed. A map switch already built it on the map
loader's `mod-ground` thread (section "Map switch: the mods' ground built by the map loader").

Change: at Startup, `modding::start_startup_ground` copies the world triangles, builds the ground on a `mod-ground`
thread and takes a `mod_ground` hold on the startup loading screen; `finish_startup_ground` (PreUpdate) hands the
finished collider over as `PreparedGround` for the current map generation and releases the hold. `ensure_ground`
installs it on the first tick, as after a switch: same triangles, same collider (`skate-dynamics` test
`a_ground_built_apart_is_the_same_as_set_ground`). A failed thread start falls back to the old build on the tick.

Tests: `the_startup_ground_is_handed_to_the_first_tick_and_releases_the_loading_screen`.

Verification (desktop, DownTown, muted, no mods): the first frame of play went from 1718 ms to 587 ms;
`modding::fixed` on it from about 1100 ms to 51 ms. The loading screen is up 0.7 s longer (4405 ms against
3691 ms), as the ground takes longer to build than the other loads. Left in that frame: `npc_sim::simulate`
483 ms with the NPC physics opt-in on (its body pool loads on the first tick, research b215).

## The NPC physics pool loads on its own thread (2026-10-18)

Problem: with the NPC physics opt-in on, the first enabled fixed tick (the first frame of play) loaded three
skater bodies for the simulated tier on the game thread: 483 ms of `npc_sim::simulate` on the desktop, 0.7 to 0.8 s
on the laptop (research b215, option A1).

Change (`living_world/npc_sim.rs`): the prewarm keeps its gate, spawn transform and count; the load runs on an
`npc-pool-loader` thread with a released `npc_worker` copy of that tick's physics (the settings, collections and
grind world a load reads), the same spawn and the shared animation source carried along. `NpcSimPool::finish_load`
joins it before anything takes a body (`spawn_sim`) or returns one (`pool_from`), so the pool order is the one a
load on the game thread gives; `simulate` takes it in without waiting once the thread is done. The log lines are the
same, logged at the join (`NPC_SKATER_SIM pool: ... (... ms, on the loader thread)`). No thread: the old load.

Tests: `pool_bodies_loaded_on_the_loader_thread_ride_like_ones_loaded_here` (ignored, data-gated: two bodies loaded
here and two on the loader path ride the same push for 240 ticks; decks, skeleton roots and states equal bit for bit
every tick).

Verification (desktop, DownTown, muted, no mods, NPC physics on): the pool loads in 569 ms on the loader thread,
done 0.7 s after the loading screen closed and 4.8 s before the first NPC switched to physics; the first frame of
play is no longer a hitch (none of frames 3 to 182 is over the hitch threshold; it was 587 ms with the startup ground
fix alone, 1718 ms before both). Left in the first second: GPU uploads (`allocate_and_free_meshes` and
`prepare_assets<GpuImage>`, one run of about 115 ms each; research b231) and `modding::fixed` 52 ms.

Follow-up (2026-10-23, USER-ASKS 295): a 146 ms frame in University (desktop session 18:25) was a skater loaded on the
game thread (`NPC_SKATER_SIM_LOAD ... 139.9 ms`) because the pool was empty: a simulated NPC that faded out at a
dead end (`npc_skaters.rs`, "reason faded out") was retired without returning its body (`apply_records`, which
returns it, no longer found it in the index), and a failed respawn (`npc_sim.rs`) dropped its body too. Both now
call `pool_from`, as the physics-error hand-back does. Check run (muted, University, 3 min, NPC physics on): 24
switches to physics, only the pool's 3 loads (on the loader thread); no fade-out happened in it, so the fade path
itself is not play-tested yet.

## The living world's models load behind the loading screen (2026-10-18)

Problem: ped, car, NPC skater and hand prop models were loaded when the first of each spawned, in the first seconds
of play: one run of about 115 ms each of `allocate_and_free_meshes` and `prepare_assets<GpuImage>` in the first
visible second on the desktop, and peds and cars appearing a few frames after they spawned (research b226 item 3,
plan b229, b231). The user
chose the trade-off (2026-10-18, "yes"): about 120 models held for the map against that upload.

Change (`living_world/preload.rs`):
- `preload_paths` builds the set from the functions the spawners call: every ped recipe a census spawn can reach
  (`PedCatalog::reachable_recipes`, which shares `model_candidates` with `look_for` so the two cannot drift) through
  `peds::glb_path`; every car of the vehicle catalog through `vehicles::car_glb` (pulled out of `car_from_record`);
  every NPC skater character through `npc_skaters::look_path` (pulled out of `present_looks`); every hand prop GLB.
  Mod looks and overrides apply, as for a spawn. Stock paths the install does not have are left out (the vehicle
  catalog lists every district's cars; a spawner that picks one fails as before).
- `preload_models` (Update, which runs behind the loading screen while game time is paused) loads the set once the
  map's data is in and holds the handles for the map; the spawners' own `server.load` of the same `#Scene0` path then
  gets the loaded asset. A startup hold `living_world_models` waits until every model is loaded or failed (cap 30 s).
  It rebuilds on a map switch or a mod look change (not on `CustomModels`, which the customizer marks changed every
  frame; an NPC look it changes mid-map loads lazily as before), and reloads only when the path set differs.
- What is shown changes in one way only: peds and cars no longer wait a few frames for their model after they spawn.

Tests: `reachable_recipes_hold_every_look_a_census_spawn_can_get` (skate-core: every look `choose` gives over 2000
seeds is in the set, also with a mod's category list and entity model), `the_preload_holds_the_paths_the_spawners_load_with_mod_overrides`.

Verification (desktop, DownTown, muted, no mods): `LIVING_WORLD_PRELOAD DownTown: 119 models`, ready in 1395 ms,
0 error lines; the loading screen is up 0.4 s longer (5029 ms against 4624 ms). The first visible second's worst
frame is 9 ms plus one 35 ms steady-state stall (before: a 65 ms frame with the ~115 ms uploads); the upload systems'
long runs now fall before the window is shown. A first run rebuilt the set every frame (`CustomModels`) and requested
missing stock car files each frame, so the loading screen waited the 30 s cap: both fixed before the commit.

## Zone ambience beds are read ahead at map load (2026-10-17)

- **Problem:** entering an ambience zone read its bed (16-bit 48 kHz stereo WAV, ~2 min, 15-29 MB) from disk on the
  game thread: session 11 (2026-10-16) had a 161.6 ms hitch with `audio_parts=[ambience:157.1 file_read:150.1]`. A
  bed was also released when it faded out, so every later entry read it again.
- **Change (research b198; outcome identical: same bytes, same start frame):** at map load or an audio content change
  the ambience system lists the bed files of every zone the map's `audio_ambience` tiles can return (the map's own
  boxes and its district's retail tiles: `MapAudio::region_tile_keys`, `Library::zone_bed_files`), reads them on the
  existing `audio-warm` worker thread (`Library::warm_files`) and keeps them loaded while the map lasts (no release
  on fade); beds that leave the set are released (`Library::release_file`). A bed not read yet is read on the spot
  as before. Memory: DownTown 6 beds (~140 MB), Industrial ~199 MB, University ~98 MB, small maps 23-29 MB; mod beds
  come in through the same manifest and go with the library rebuild on a mod toggle.
- **Tests:** `zone_beds_read_ahead_give_the_same_bytes` (game_audio library tests); game_audio tests 81 pass. Muted
  DownTown run: `AUDIO_AMBIENCE beds read ahead for DownTown: 6`, the zone change dt_less_busy -> dt_main logs no
  ambience / file_read hitch; the map-load frame keeps 15 ms of ambience (the first bed starts 1-2 frames after load).
- **Open (b198):** other game-thread reads in the audio path (crossfade layouts once per map, world speech, mod
  audio): listed in research b198.

## In-game Refresh Game Files ended with exit code 1 (2026-10-18)

- **Report:** laptop sessions 2026-10-18 09:55 and 12:17: both refreshes ended with setup exit code 1; setup had
  published a new installation each time.
- **Root cause (research b236, verified the lines):** Refresh Game Files restarts the game with its arguments
  plus `--refresh-assets` (`setup.rs` `restart_with_refresh`); the restarted copy's setup step reads it and repoints
  the assets link, then the argument loop in `config.rs` had no case for it and returned "Unknown argument", so the
  game exited with code 1 right after a successful refresh. The crash report hid the reason (its filter drops lines
  with "unknown argument" and paths).
- **Change:** `config.rs` accepts `--refresh-assets` (read earlier by setup); the launcher's Refresh result shows
  setup's own failure line (`GAME_FILES failed: <reason>`) with the exit code instead of "see above".
- **Verification:** not run (a run with the flag starts a real refresh); the parser has no unit test (it reads the
  process arguments directly). The next in-game refresh should start the game again; its
  `logs\game-<stamp>.stderr.log` shows the lines if not.

## Fixed-step cost readout (`SKATE_FIXED_TIMING`, 2026-10-18)

Problem: `SYSTEM_TIMING` sums every schedule and the render app per frame and lists the top 16, so the fixed step's
cost (b213: about 4-5 ms of a ~5-6 ms laptop step not split by system) could not be compared with `fixed_ms`.

Change (`crates/skate-game/src/system_timing.rs`, research b214 / b268):
- `SKATE_FIXED_TIMING=1`, also on under trace-all, extends the same layer: the fixed step is the span of
  `FixedMain::run_fixed_main` (one per step, single-threaded); while it is open, `system` and `system_commands` spans
  of the fixed schedules (membership read from `Schedules`, again whenever their system count changes) are summed.
- Once per second: `FIXED_TIMING steps= step_ms=mean/max busy_ms= gaps_ms= cpu_ms= commands_ms= unlisted_ms= |
  <every fixed system>=mean ms per step/max ms x runs ... | unlisted <top 3>`. `busy` = wall time with at least one
  fixed system or command application running, `gaps = step - busy` (executor, sync points), `cpu` = the sum of the
  systems (parallel systems overlap, so it can exceed the step), `unlisted` = other spans inside the window (render
  systems overlapping the step, run conditions).
- Off: nothing changes (the layer is installed only when either readout is on; `SYSTEM_TIMING`'s line keeps its form,
  command spans are not in it).
- Verification: test `the_fixed_step_system_is_found_by_name` pins the step system's name; trace-all tests pass
  (`SKATE_FIXED_TIMING` in its switch list). Used in the fixed step executor runs (2026-10-24, "Fixed step executor").
- Open: nested spans (physics phases, per-mod callbacks) are not split (v2). The run tool
  (`tools/claude-skills/optimization-pass/tools/frame-time-run.ps1`) keeps the whole stderr log, FIXED_TIMING lines included.

## Launcher in a window; the launcher and the game come to the front; a steady splash (2026-10-19)

- **Problem (user, 2026-10-19):** the launcher should be windowed instead of full screen and open in the
  foreground, and the game it launches should come to the foreground too once the splash is done
  loading. Also, the startup splash resizes itself oddly depending on the resolution settings.
- **Causes:**
  - The launcher form was borderless and maximized (`SkateLauncher.cs`), and maximized again after every job.
  - Windows only lets the foreground owner hand the foreground on. A window that Steam or a hidden launcher starts
    stays behind, so plain `SetForegroundWindow` failed silently (see also the 2026-10-16 window notes in the
    steam-launcher skill).
  - The launcher brought forward the first window the game showed. That is the startup splash; the game's own window
    comes later and was never brought forward.
  - The game closed the splash in the same frame it asked for its window to be shown. The splash could go before the
    window was up, so the foreground went to whatever was behind it and Windows refused the game's focus request.
  - The splash was created before the renderer makes the process DPI aware, so it was a DPI-unaware window that
    Windows could rescale during the start; its paint also laid out to the client size it found.
- **Change:**
  - Launcher: a normal resizable window at 70 % of the work area (16:10, centered, at least 800 x 540; the layout
    already scales with the window). `Native.Front` joins the foreground thread's input for the call, then
    `BringWindowToTop` + `SetForegroundWindow`; used at start, after a job, and for the game.
  - Launcher: while a game runs it brings forward every new visible main window of the game for 5 minutes (the
    splash, then the game's window), not only the first.
  - Game (`startup_loading.rs`): the splash closes 3 frames after the window is shown and focused.
  - Game (`splash.rs`): the splash thread sets its own per-monitor v2 DPI context before creating the window, sizes
    the window by the system DPI, draws the 560 x 240 layout into a fixed bitmap and scales it to the window
    (HALFTONE), so a later DPI or display change cannot re-lay it out.
- **Verification:** the launcher compiles with csc 4.0 (C# 5); skate-game builds and its suite passes. Not
  play-tested yet: the foreground behavior needs Steam and Windows Terminal (test it there, never only in a plain
  console).
- **Open:** whether the splash resize the user saw came from DPI or from the game's resolution settings is not
  proven; the next session with the splash says.
- **Other platforms (2026-10-22, PR #52 review):** the splash links `user32` / `gdi32` / `kernel32` and the refresh
  log's `local_stamp()` (`setup.rs`) links `kernel32`, which broke the link of Linux and macOS builds. `splash` is now
  `#[cfg(windows)]` with a no-op stand-in module of the same calls (`show`, `close`, `stage`, `detail`, `warning`,
  `active`) elsewhere, so a port can put its own screen behind them; `local_stamp()` keeps `GetLocalTime` on Windows
  and builds the same stamp in UTC from `SystemTime` elsewhere (checked against Python's calendar over 20,000 times).

## An assets link to a removed installation repairs itself at start (2026-10-19)

- **Problem (laptop note 2026-10-19):** checkouts and launcher versions share one data folder. A Refresh Game Files in
  one of them publishes a new `installations/<id>` and removes the old one, so every other `assets` junction pointed
  at a folder that no longer existed until the next launcher start (`Repair-AllAssetLinks`). Started any other way
  (scripts, `--check-assets`), the game stopped at once: "Asset root assets: The system cannot find the path
  specified. (os error 3)".
- **Change (`setup.rs`):** before the asset root is read, a `--assets` link whose target is gone is followed to its
  data folder (`<data>/installations/<id>/assets`), and when that data folder has a current installation the link is
  re-pointed at it (`SETUP: ... pointed at a removed installation` in the log). A working link and a real folder are
  never touched; a failure only logs and the start goes on as before.
- **Tests:** `a_link_to_a_removed_installation_is_repointed`.

## Launcher-only download (2026-10-19)

- **Problem:** the Skate launcher needed a full checkout of the repository around it, so a player who only wants to
  play the fork's versions had to clone and set up a dev checkout first.
- **Change (`tools/steam-launcher/launcher.ps1`, README "Launcher-only download"):** a `STANDALONE.txt` next to the
  script makes the launcher standalone. Its config, state and versions stay in the launcher folder; each version is a
  `git clone --single-branch` of only its branch into `versions\<id>`, made the first time it is played or built, and
  later updates fetch that branch and fast-forward as before. `builtin_versions` in `config.json` limits the built-in
  list. The first version's `data\` holds the game files that every version shares, and logs go to the selected
  version's `logs\`. The fork release workflow packages the launcher files plus `STANDALONE.txt` as
  `skate-launcher.zip` (with `.sha256`) on the rolling release and on tagged releases.
- **Verification:** the script parses (0 errors); a dry run in a scratch copy listed the versions and entries without
  downloading; the clone command and the update fetch / fast-forward were run once against the fork's
  `hails-additions` (6 s, 0.1 GB, only that branch) in a throwaway folder that was then deleted; the workflow's
  packaging step was run locally up to the zip.
- **Open:** the first build inside a fresh standalone folder (setup, game files) is untested on a clean PC.

## Launcher: screen and selection animations (2026-10-19)

- **Ask:** the user: launcher menu animations when changing screens and clicking items, not overdone,
  since the instant screen switches are a little jarring.
- **Change (`tools/steam-launcher/SkateLauncher.cs`):** a new screen slides in by 3% of the window width (from the right
  when opening, from the left when going back) and fades up from the backdrop over 180 ms, eased out. The top bar
  stays put. The selection ring grows from 1 to 3 px over 120 ms instead of snapping on. A screen counts as new when
  its depth or title changes, so status refreshes of the main page do not animate. Input is never blocked: the
  animation only changes what is drawn, and the timer repaints only while one runs.
- **Verification:** compiles with the .NET Framework 4 `csc` that `build.bat` uses (no warnings). Not looked at on
  screen yet; the user judges the feel.


## Start-up splash: out-of-date game files are named at once (2026-10-19)

- **Ask (user, laptop session 2026-10-19 10:08):** the refresh warning should show under the startup splash once
  detected during the startup. The asset check ran about 24 s after the game window opened (it waits for the
  loading screen so its thousands of reads do not slow the start on an HDD), so the user only noticed "central
  message HUD missing" / "ped chase alert textures missing" after playing.
- **Change:** `asset_check::quick_check` (the feature files later builds added: the central message HUD, the chase
  alert textures, the hand props' models and grip points; a few file tests and one 17 KB read) runs in `main.rs` right
  after the settings load, while the splash is up. Findings become `splash::warning`: an amber, two-line, wrapped
  line under the status ("Game files out of date: <what>. Refresh Game Files (the launcher's Tools, or Extras in the
  game) to turn them on."), kept until the splash closes; the finished lines make room (two kept). Logged as
  `ASSET_CHECK start-up: ...`. The full check after loading is unchanged (it still offers the refresh in game, and
  out-of-date menus still force it).
- **In game (user, 2026-10-19):** a clear, easy to read red warning box in the fps counter's style that says an
  asset refresh is needed, below the fps counter when it is shown and in its place when it is not. The frame-time counter and a red box share a top-left column (top right until 2026-10-23, when the HUD minimap took that corner) (`frame_timing` `spawn_overlay`): the
  box sits below the counter, and a hidden counter takes no room, so the box then sits in its place. Same padding and
  font as the counter (and its Simple-mode size), dark red, white text:
  `GAME FILES NEED A REFRESH` / `Off until then: <features>` / `Launcher: Tools > Refresh Game Files (or Esc >
  Extras)`. Its source is `asset_check::publish`: the start-up quick check, replaced by the full check when it ends
  (a clean full check hides it). Findings are named as features (`feature_name`: "chase messages (ZAP!, BAM!)",
  "ped chase alerts", "ped hand props", "menus", else "<group> files"); the splash uses the same names.
- **Tests:** `the_splash_names_what_stays_off_until_a_refresh` (asset_check: splash text and box text),
  `systems_touch_only_their_own_state` (frame_timing; the box is written by `show` only). Not looked at on screen yet.

## Frame pacing: hitches counted against the expected frame, a cheaper fixed step (2026-10-19)
- **Problem:** laptop session 2026-10-19 11:05 (user: hitches were still fairly common in testing, but
  none large enough to stutter the game): 567 `FRAME_HITCH` lines. The frame log shows frames alternating about
  5 ms and 11 ms at ~124 fps: every other frame also runs the ~6 ms fixed physics step (60 Hz). 9,912 of the 9,918
  frames flagged as hitches were such normal one-step frames (2 x the median of all frames is ~11 ms).
- **Hitch rule (ours, diagnostics only):** a hitch is now a frame longer than 2 x its EXPECTED time = the median of
  the previous 120 frames without their physics loop, plus the frame's fixed steps x the median cost of one step
  (`frame_timing::stats::expected_ms`). The overlay, `FRAME_HITCH`, the `SKATE_FRAME_LOG` `hitch` column and the mod
  snapshot use it; the log's `median_ms` column / field now holds the expected time. Overlay bars turn red only for a
  bucket with a real hitch. Gameplay is unchanged (the plugin still writes only its own state).
- **Cheaper step (user's choice "Cheaper physics step" over a pacing limiter):** profile on the main PC (release,
  DownTown, no input, no mods, Chrome trace, 3,600 steps): 3.84 ms per fixed step; skater collision + solve 1.74 ms
  (player + simulated NPC skaters), `npc_sim::simulate` 1.03, `physics::advance` 0.90, the mods' fixed step 0.50 of
  which the Rapier step 0.37 (with no mod installed), `think_peds` 0.35. Changes so far:
  - The mods' Rapier world steps only while a mod is running or something a mod made exists
    (`Mods::needs_dynamics_step`, the same rule as the snapshot reader). Without mods it held only the static ground
    and the skater proxies, re-posed every tick. Not byte-identical for a mod's first contact ticks (Rapier's
    carried-over contact state of the proxies); the user accepted that.
  - Simulated NPC skaters no longer build the mods' per-tick contact frame (`player_physics::before_solve`,
    `after_solve`, `joint_loads`): only the player's is read (`player_physics::snapshot`), the builders take shared
    references and only fill `mod_contact_frame`, so physics is identical (research b327: ~0.2 to 0.35 ms a frame).
  - The NPC worker sync copies a proxy set per job each tick: `Proxies` now has a field-by-field `clone_from` that
    reuses the buffers (the derived one allocated every Vec again; same values; test `clone_from_equals_clone`; b333).
  - The grab registry (~700 DownTown props and cars, rebuilt three times a tick) checks duplicate ids with a set
    instead of scanning the earlier objects (O(n^2)); the same error at the same object (test
    `registry_errors_match_the_old_duplicate_scan`, 2,000 random lists against the old scan; b334).
  - The drive filter returns at once when no mod disabled a drive (no per-tick rebuild of three Vecs; b327).
- **Files:** `crates/skate-game/src/frame_timing/{mod.rs,stats.rs,hitch.rs,log.rs}`, `crates/skate-game/src/modding/mod.rs`,
  `crates/skate-game/src/physics/{solve.rs,network.rs}`, `crates/skate-game/src/modding/player_physics.rs`,
  `crates/skate-core/src/player/offboard/grab_scene/registry.rs`.
- **Tests:** `fixed_step_frames_are_expected_not_hitches`, `alternating_fixed_step_frames_are_not_hitches`,
  `dynamics_step_only_with_something_a_mod_owns`; the trace-all test's report window is 1 ms now (a 20 ms window could
  close after its ~20 ms run: flaky).
- **Open:** a measured after-profile; further exact candidates in research b327
  (the `solve_queries` timer also covers the prop step and mod maps, so split it before optimizing "queries").

## Code reference: frame_timing/hitch.rs

Detail moved out of the code comments of `crates/skate-game/src/frame_timing/hitch.rs`; each comment there links its section here.

### Frame hitch line

`FRAME_HITCH` log line: one line per hitch frame (rate-limited), so a reported stutter can be placed and attributed from the session log alone (logs must diagnose).

A frame is a hitch when it took longer than `factor` x its expected time (the overlay's rule, [`super::stats::expected_ms`]: the median frame without physics plus its fixed steps x the median step cost; the line's `median_ms=` is that expected time) or longer than `floor_ms`. The line carries the frame's split (main-thread CPU, physics loop, wait outside the schedules), the wall time of the main stages (fixed: input / controls / physics, and inside the physics tick: animation graphs, collision and solve, finish, scoring; frame: assets / physics present / animation / audio pass), the game thread's wait for the audio render lock, the player (state, airborne, trick, position, tick), the entity count and the audio load (mixer voices, AEMS instances of the last block).

Observation only: the marker systems write only [`HitchSpans`] and read clocks; the phase timers are relaxed atomics around the physics phases. Gameplay is identical with or without it.

Thresholds are data ([`HitchConfig`], defaults below), overridable without a rebuild through `SKATE_FRAME_HITCH` (`off`, or `factor=2,floor_ms=50,interval_s=0.5`).

### Render side split

(2026-10-24, USER-ASKS 299; todo `render-stall-attribution.md`.) Session 2026-10-23 13:39 had a 701.8 ms hitch with `wait_ms=699.1 slowest=post_render` (bench grind at University) and nothing in the log said what the render thread did. Bevy renders frame N-1 on its own thread while the main thread runs frame N; the main thread's end-of-frame extract waits for it, so a slow render frame shows up as `wait_ms` / `post_render`.

`frame_timing/render_split.rs` adds `render=[...]` to every `FRAME_HITCH` line, summed over the render frames that finished since the previous main frame (normally one, the frame the main thread waited for):

- `frames`, `frame_ms` (render schedule wall time), `max_frame_ms`;
- `prep_ms`: extract commands to the end of `Prepare` (asset preparation, views, queue, sort, buffers and bind groups);
- `graph_ms`: the `Render` set, which is the pipeline queue (`PipelineCache::process_queue`) plus the render graph, submit and present;
- `pipelines_new` (pipelines added to the cache), `pipelines_compiling` (most in the `Creating` state at once), `pipelines_done` (compiles finished);
- `meshes`, `mesh_kb` (vertex bytes), `image_kb`: assets extracted for upload that frame.

A stall with a large `graph_ms` and pipelines compiling or done is a first-use pipeline compile; a large `prep_ms` with `mesh_kb` / `image_kb` is an upload; a large `graph_ms` with neither is a GPU or present wait (then `SKATE_PERF_GPU=1` for pass times).

Always on and observation only: five marker systems on the edges of Bevy's chained render sets (`ExtractCommands`, `Prepare` / `Render`, `Cleanup`), so they add no ordering between render systems (unlike the opt-in `SKATE_PERF_RENDER` split in `performance.rs`, whose inner markers can). The markers write only their own render-world resource and relaxed atomics; pipeline states are counted only while the cache has waiting pipelines or grew.

Open: the main thread's own extract time is still inside `wait_ms` (no marker in `ExtractSchedule`); the bench stall's cause needs a session with this line.

## Code reference: trace_all.rs

Detail moved out of the code comments of `crates/skate-game/src/trace_all.rs`; each comment there links its section here.

### Trace all

`SKATE_TRACE_ALL=1`: one play session records every session trace and diagnostic the engine has, whatever the launcher remembered to set.

[`apply`] runs first thing in the game process (before any thread, before the log subscriber) and switches on every diagnostic below by setting its own environment switch, so every existing reader (including the ones cached in `OnceLock`s deep in audio and physics) sees it unchanged. Output files the launcher did not name go next to the ones it did (or `<exe dir>/logs/trace-all- <unix seconds>`). Diagnostics only: nothing here changes the game (see `frame_timing::tests::game_is_identical_with_trace_all_on_or_off`).

What trace-all does differently from the single switches, so a whole session stays cheap: the performance report rolls for the whole session instead of exiting after 25 s (`performance.rs`), GPU timestamp queries are used only when the adapter has them (`app.rs`) and only on one frame in `SKATE_PERF_GPU_EVERY` (30 by default, `performance.rs`), the render phase split is sampled in windows (`performance.rs`), every log line goes through a bounded writer thread instead of a blocking stderr write (`profiling.rs`), and a few per-session lines log more often (`HELD_PROP`).

## Code reference: system_timing.rs

Detail moved out of the code comments of `crates/skate-game/src/system_timing.rs`; each comment there links its section here.

### System timing

Opt-in per-system cost readout (`SKATE_SYSTEM_TIMING=1`, also under trace-all): once per second one `SYSTEM_TIMING` log line with the systems that took the most wall time, as mean ms per frame, the longest single run and the run count. Bevy opens a `system` span (field `name`) around every system run (the `trace` feature is always on, research b214); this layer names each span once when Bevy creates it and adds up the time between enter and exit per name, with no allocation per run. Parallel systems overlap, so the per-system numbers add up to more than the frame; they show where the CPU goes, the frame log shows the frame. Off: the layer is not installed at all.

`SKATE_FIXED_TIMING=1` (research b214 / b268): the same spans inside the fixed step only, once per second one `FIXED_TIMING` line with the step's wall time, the time any fixed system or command application ran (`busy`), the rest (`gaps`: executor and sync points), and every fixed-schedule system per step. The step is the span of `FixedMain::run_fixed_main` (one per step, single-threaded); fixed systems run on pool threads with no span parent, so the window is a global flag, and render systems that overlap it are kept apart by schedule membership.

## Code reference: physics/manual_landing_log.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/manual_landing_log.rs`; each comment there links its section here.

### Manual landing log

`MANUAL_LANDING` diagnostic line, one per landing (logs must diagnose: landings that "should have" gone into a manual and did not). Engine logging only, no gameplay effect.

- **Asked for:** the Manual intent (the stick's forward / back value) on the landing tick, how long it was held before touching down, and the motion graph's `ManualEngageTime` intent at touchdown (retail's motion graph only takes the Manual transition with `HasIntent ManualEngageTime > 0.2`, from InAirStatic / BoardAdjusts).
- **Where the graph was:** the motion graph state (id and authored name) and the playing clip at touchdown, so a landing from a trick or grab state (no Manual transition) is visible.
- **Granted:** the manual (balance) state flag (`+340`) comes on within [`WINDOW`] after the landing. "Why not" names what the log can see: no stick, the stick let go before the wheels came down, or the graph declined it with the stick held.

## Code reference: game_audio/hitch_parts.rs

Detail moved out of the code comments of `crates/skate-game/src/game_audio/hitch_parts.rs`; each comment there links its section here.

### Audio hitch parts

Per-system wall time of the game audio's frame systems, summed per frame and read (and reset) by the `FRAME_HITCH` line (`frame_timing/hitch.rs`), so a hitch names the audio system that took the time (audio stage hitches of 150 to 870 ms while towed fast had no log event to explain them). `bank_load` is the game-thread part of `Native::ensure_bank` (inside `emitters` and the other callers).

## Code reference: app.rs, performance.rs, frame_timing/stats.rs

- `app.rs` `wgpu_features`: trace-all (`SKATE_TRACE_ALL=1`) never requires the GPU timestamp features: Bevy's default
  `Functionality` priority already requests every feature the adapter has, so the queries are on where the GPU
  supports them and device creation cannot fail where it does not (`log_gpu_timing` says which).
- `performance.rs`: with trace-all the harness never exits; it rolls for the whole session in windows of
  `ROLLING_WINDOW` seconds, a writer thread rewrites the JSON after every window (each window's summary plus the newest
  window's render diagnostics; per-frame samples are in the frame log); the game thread only pushes one sample per
  frame into a preallocated buffer and hands the full buffer over once per window.
- `frame_timing/stats.rs` hitch: a frame longer than `HITCH_FACTOR` x its expected time (needs `HITCH_MIN_HISTORY`
  previous frames). Expected = the median of the previous `HITCH_HISTORY` frames without their physics loop, plus the
  frame's fixed steps x the median cost of one step (`expected_ms`). A frame that runs the ~6 ms fixed step every other
  frame at ~180 fps is a normal frame, not a hitch (2026-10-19 laptop log: 9,912 of 9,918 flagged frames were that).

## Logging off the game threads (2026-11-12)

**Problem.** Major hitching on the desktop (0.3 to 4.6 s frames, worst on tricks); the laptop much less.

**Root cause.** Every log line was written to stderr synchronously from the thread that logged it, and the desktop test's log file sat on a hard drive. Stack snapshots (cdb) caught the main thread in `NtWriteFile` inside the tracing writer three times out of three; after the tracing lines moved to the queued writer, a Chrome trace showed `crash_context::sample` (in `Last`, on every state change, so on every trick) waiting 0.2 to 1.9 s on the stderr lock for its `eprintln!` report lines. About 300 `eprintln!` sites write to stderr directly.

**Change.** `profiling.rs`: (1) the bounded log writer thread is always on (it was trace-all only); (2) `stderr_pump` (Windows): the process's stderr is an 8 MB pipe that its own thread forwards to the real stderr, so any stderr write in the process is a memory copy; a write waits only when 8 MB are already queued. Both drain on exit and on a panic (`drain_log_queue`). The Chrome trace writer is unchanged.

**Files.** `crates/skate-game/src/profiling.rs`, `crates/skate-game/Cargo.toml` (windows crate features Win32_Security, Win32_Storage_FileSystem, Win32_System_Console, Win32_System_IO, Win32_System_Pipes).

**Verification.** Desktop contest runs street_dwtn_01: before, 98 hitches in 2 min with 27 over 500 ms; after both changes, 13 hitches in 5 min, 11 under 50 ms; in play the hitching was gone apart from small blips.

**Linux / macOS (2026-11-12, all-OS rule).** The same pump: fd 2 becomes a pipe's write end (`dup2`), a thread forwards the read end to the original stderr (`dup` of fd 2), `FIONREAD` for the drain; Linux asks for an 8 MB pipe (`F_SETPIPE_SZ`, else 1 MB, else the kernel default), macOS keeps its fixed pipe size. `libc` (=0.2.189, the version already in the lockfile) as a Unix-only dependency. Checked: the module compiles for `aarch64-apple-darwin` (a scratch crate with the module and `libc`; the whole game does not cross-compile here, its C build scripts need the macOS toolchain); not run on Linux or macOS yet (the laptop / a Linux machine).

**Fix: `--check-assets` never exited (2026-11-12).**
- Problem: on the laptop's push check of hails-additions b9c8bdf5, `skate3rust.exe --check-assets` ran 73 minutes with no output until it was killed. On the desktop it printed `SKATE_ASSETS_READY` and then never exited.
- Root cause: the Windows drain asked the pipe's read end how many bytes were waiting (`PeekNamedPipe`). A synchronous pipe handle runs one call at a time, so that call waited behind the pump's own `ReadFile`, which does not return while nothing is written. The drain's time limit never got a turn, and the process hung in the profiling guard's drop after `main` returned. A panic would hang the same way (the panic hook drains too).
- Evidence: file markers on the exit path stopped right after the log queue drain; with the pump switched off (a temporary experiment) the check exited with code 0.
- Change: on Windows the drain now ends the pump. It restores the original stderr, closes the pipe's write end, and waits (within the limit) until the pump has forwarded the rest and read end of file. Nothing queries the read end any more. Linux / macOS keep `FIONREAD` (no such serialization).
- Verification: three desktop runs of `--check-assets` against the dev install exited with code 0 in 2.4 to 2.9 s, each with every line through `SKATE_ASSETS_READY`.
