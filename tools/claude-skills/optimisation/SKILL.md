---
name: optimisation
description: Make engine code faster WITHOUT changing behaviour - measure first, prove output is identical, and put every change through adversarial review before it lands. Use for any performance work (stutter, frame time, audio/physics/render cost, load times), and for further passes over the native audio.
---

# Optimisation

Built from the native-audio optimisation passes of upstream PR #32 (merged): the tools, baselines,
proofs, pitfalls and the review checklist that worked there. Helper scripts are in `tools/` next to this file
(installed under `<repo>/.claude/skills/optimisation/tools/`; the shell scripts find the repo root from there).

## Rules (binding)
- **Optimise only with proof of identical behaviour** (skill `regression-check` section 3b). Byte-identical output
  is the bar. A tolerance needs an explicit reason and the project owner's OK. Never reassociate floats, never swap
  `mul_add` for `a * b + c` (or the reverse), never change the order of RNG draws.
- **Measure first, then change.** No change without a baseline number for the thing it should improve, measured the
  same way after it, with repeats (noise).
- **Worst case matters more than the average:** p99 / p99.9 and the per-second maximum, not only p50. A change that
  improves p50 and worsens p99 is a regression.
- **Gameplay data comes from real play sessions** (state logs recorded while a person plays), not scripted skating
  runs. Benchmarks are headless. Data with malformed rows is discarded, not parsed around.
- Do the adversarial review yourself and write it down: concrete counter-examples tried, not "looks fine".
- Product-level trade-offs belong to the project owner: raising the CPU baseline (`+fma`), adding `unsafe` to a
  crate that forbids it, trading memory for frame time, adding threads. Measure them, then report them.

## Process
1. **Reproduce and measure in real play.** Read the `AUDIO_TIMING` lines (`SKATE_AUDIO_TIMING=1`, one line per second
   in `logs\game-*.stderr.log`: `name=max/avg us x count`). Summarise per system (per-second max median / p90 /
   worst, average, calls per second) with `tools/audio_timing_summary.py LOG...`. Correlate spikes with the log lines
   around them (for example `AUDIO_EMITTER start ... (native)`).
2. **Reproduce headless and repeatably.** `tools/e2e_bench.sh LABEL [row|fps300]`: the e2e scenarios plus whole
   play sessions replayed through the e2e harness, timed per game-thread call and per render block. Every output is
   hashed. Inputs:
   - `.local/audio-re/opt/scen/*.tsv` (the synthetic e2e scenarios);
   - `.local/audio-re/opt/real/*.tsv`, made with `py -3.13 .claude/skills/aems-port/tools/e2e/scenarios.py
     --from-log <state log> --cut r0-999999 real_X .local/audio-re/opt/real`. Check that the row count equals the
     log's lines - 1: `from_log` silently drops malformed rows.
3. **Find the cause** before touching code. Use temporary per-stage timers: a throw-away `prof` module with
   thread-local `Instant` accumulators around each stage, printed per scenario by the e2e test. Also count events
   (voices per block, kernel samples per path, zero-input blocks, settled filters). Write the numbers down. Remove
   all of it before landing (keep a pristine copy of the sources). Micro-benchmarks for single questions: small
   `.rs` files built with plain `rustc -O`.
4. **Change one thing at a time.** Keep the old implementation as a test-only reference and compare bit for bit on
   synthetic edge cases (see the checklist), and in the e2e hashes.
5. **Prove identity:** `tools/compare_runs.sh base NEW` must print IDENTICAL for every set and mode. Also run
   `cargo test -p skate-audio --release --locked`, `cargo test -p skate-game --release --bin skate3rust --locked --
   game_audio::`, and the `--no-run` build.
6. **Adversarial review** (checklist below). Revert anything not proven.
7. **Re-measure** with repeated runs (at least 2 before and 2 after), and compare p50 / p90 / p99 / p99.9. Maxima on
   a shared machine are OS noise: they move between runs and block indices. Don't claim them.
8. **Document** in the change's docs entry and the PR: numbers, proofs, rejected ideas.

## Tools (in `tools/`)
- `e2e_bench.sh LABEL [MODE...]`: builds into `.local/opt-target`, its own target dir, so it never waits on or
  invalidates the main build. Renders scen + real in `row` (one host call per 60 Hz row) and `fpsN` (the game's
  host at N fps, console MixMap cadence) modes. Writes `.local/audio-re/opt/runs/LABEL/<set>-<mode>/{timing.txt,
  hashes.txt}`. `LABEL=base` keeps the `.f32` renders (large); other labels delete them unless `KEEP=1`. Honours
  `CARGO_TARGET_DIR` / `RUSTFLAGS` for experiments.
- `compare_runs.sh BASE NEW`: hash identity per set and mode, then `timing_summary.py`.
- `timing_summary.py RUN...`: pools `*.ours.timing.tsv` (written by the e2e harness when `E2E_TIMING=1`) and prints
  n / p50 / p90 / p99 / p99.9 / max / sum per run.
- `audio_timing_summary.py LOG...`: the real-play `AUDIO_TIMING` readout, incl. the render load gauges
  (`block_voices` / `block_instances` / `block_grains`).
- `frame_log_summary.py LOG [--hitches N]`: in-game frame time. Enable with the graphics menu "Frame-time counter"
  or `SKATE_FRAME_LOG=<file>` (per-frame TSV: frame_ms, fixed_steps, fixed_ms, main_ms, hitch). Prints
  percentiles, per-second worst, physics catch-up frames, and the longest hitches with UTC time and main-thread vs
  render/GPU attribution. Use this first for any stutter report.
- Allocation check: `crates/skate-audio/tests/render_alloc.rs`, a counting global allocator (thread-local switch)
  around steady-state `Runtime::render_block` on a full scene. It asserts 0 allocations. To find an allocation site,
  print `Backtrace::force_capture()` from the allocator with counting switched off around the print.
- Baselines of other modes later: swap the pristine sources in, run `e2e_bench.sh base_x MODE...`, restore the new
  sources and confirm with `diff -rq`. Keep the tree compiling throughout: both versions build.

## Review checklist (do every item; write down what you tried)
- **Purity:** does the shortcut rely on a function being pure? Check that it reads no hidden state: caches, RNG,
  counters, `self` fields outside the arguments. Key memos on the exact bits (`to_bits`), never `==` (-0.0 == 0.0,
  NaN != NaN).
- **Signed zero and NaN:** `+0.0` vs `-0.0` inputs; NaN / inf coefficients and histories; a NaN parameter. `0*x` is
  +-0 and `+-0 + c` is c only for finite x. Check at run time where possible.
  Example: `kernel_silent_tail` evaluates the three feed-forward forms once and requires the bias bit pattern.
- **Position dependence:** retail kernels associate by position (the biquad's `n % 8` groups). Any block-level
  shortcut must keep group alignment: repeat whole groups, and continue at a multiple of 8.
- **Block sizes:** 0, 1, 7, 8, 9, 15, 16, 24, 255, 256, 264.
- **State across blocks:** filters settling then being disturbed, coefficient changes (jitter every console frame,
  re-rolls), bypass on/off (history clear), map reloads / bank unloads, voice stealing / `max_voices`, releases
  mid-block.
- **Frame-rate extremes:** e2e `row` and `E2E_FPS=300`. Add `E2E_FPS=30` / `1000` when host-side code changes.
- **Integer identities:** a modulo replacement must equal `%` for every input, overflowed ones included.
- **Allocation order and RNG order:** turning a `Vec` into an array must keep the evaluation order of any
  side-effecting closure. Use an explicit loop for RNG draws.
- **Is the speed-up real?** Repeat runs and report the spread. Check whether the cost moved to another thread or to
  the game thread, and whether p99 got worse. Profile instrumentation itself costs time: compare like with like.

## What the audio passes found (worked examples)
- **The cost was `f32::mul_add` as a library call.** Without `+fma` every fused multiply-add is a call into the CRT's
  `fmaf`: roughly 3x slower per biquad channel than hardware FMA. Results are bit-identical. Enabling hardware FMA
  needs `-C target-feature=+fma` (raises the CPU baseline) or runtime dispatch. Done as runtime dispatch: a crate
  (`skate-audio-fma`) writes each per-sample `mul_add` loop once (`#[inline(always)]`) and wraps it as `plain::*` and
  `#[target_feature(enable = "fma")] fma::*`, selected after `is_x86_feature_detected!`, cached in an `AtomicU8`,
  `SKATE_AUDIO_FMA=0` forces plain, one `unsafe` call with a `// SAFETY:` comment. Render p50 roughly halved; all
  e2e outputs byte-identical on both paths.
  Numbers: about 1.4 us per 256-sample biquad channel through `fmaf` vs 0.43 us with hardware FMA, about 19 ns vs
  1 ns per FSS `sin`; bit-identity checked on 100 M random cases and the whole e2e set. A safe software fma (f64
  product + sum, with a midpoint check and a library fallback) is exact but no faster (latency-bound). Only the
  per-sample loops moved to the dispatch (biquad kernel and silent-tail feedback, FSS sin / cos / oscillator,
  resampler); modules that call `mul_add` once per block or per parameter change gain nothing. Render p50 went from
  129 to 54-58 us, p99 from 286-316 to 158-209 us (real sessions). Proof tools: `skate-audio-fma/src/tests.rs`
  (both copies directly) and `skate-audio/tests/fma_paths.rs` (the whole runtime on both paths via `force_fma`, in its
  own test binary because the path choice is process-wide).
- First pass totals: render per 256-frame block p50 210 to 130 us, p99 about 360 to 233 us, steady-state render
  allocations about 104 per block to 0; all e2e outputs and voice logs byte-identical (13 scenarios + 4 whole
  sessions, row and fps300). Second pass (allocation-free render, direct source read in `Voice::render`, mixer
  voice scratch planes, fewer game-thread allocations): full world render p50 -11 to -14 %, player sessions p50 -1 to
  -4 %.
- Not attempted, and why: merging game-thread lock sections (it changes which intermediate states a render can
  see); skipping silent grain chains (their bias-level output reaches the master bit for bit).
- **NaN payload is the one FMA difference:** with several NaN operands, which NaN propagates depends on operand
  order. Compare with NaNs mapped to one marker and count payload differences. LLVM constant-folds edge-value calls
  with its own NaN: feed test inputs through `std::hint::black_box`.
- Exact shortcuts that worked: a settled filter on an 8-periodic block (run one group of 8; if the history comes
  back bit-identical, repeat it); silent tails (only the feedback FMAs); twin silent channels with equal histories
  (copy); trig reuse when a lane repeats; `%` -> a proven `wrap` in ring buffers; stack arrays instead of per-voice
  `Vec`s; reused pick buffers; an allocation-free render (pools, stack buffers, in-place loops).
- Dropped because the gain was not real: dispatch hoisting in the evaluator walk (exact, but no faster); a
  silent-bus memo (exact, but its key compares and silence scans cost what the skipped EQs saved).
- Lessons:
  - The e2e real-session replays are the best identity net (they hit jitter, re-roll, settling and silent-bus paths
    that synthetic scenarios don't).
  - Count paths (how many blocks take each shortcut) before estimating gains, then still measure.
    Example: the fused HPF -> LPF cascade idea died at the count (both filters are active in only 1-1.4 % of voice
    renders); the silent-bus memo would have hit 38 % of bus blocks and still lost.
  - Exact code motion in an interpreter loop buys nothing; only a different program representation would.
    (The evaluator walk is about 13 ns per op over about 2,700 ops; removing the second dispatch and extra lookups
    did not move it.)
  - Make an allocation test fail on the old code first.
    (`render_alloc` counted 1,179 / 5,061 allocations on the pre-pass evaluator and 0 / 0 after.)
  - Pool shape matters: zero-capacity lists re-allocate on every post; use best fit and give new lists a little room.
  - A data-gated test run needs `--no-fail-fast`, otherwise later test binaries never run and there is nothing to
    diff.
  - Proof per change on a cumulative tree: if each state is identical to the reference, so is every step.
    Diff the data-gated tests' printed tables too, minus the timing lines.
  - Time with interleaved saved binaries when other work shares the machine (A B A B rounds); name disturbed rounds
    and leave them out. Benchmarks taken while a build runs are worthless.
    Save the e2e test exe after each build (`<target>/release/deps/skate3rust-*.exe`) and run the saved exes in
    A B A B rounds; the exe needs the toolchain's `bin` (std DLL) and the target's `deps` on `PATH` (POSIX form in
    Git Bash). One disturbed side typically jumps 15-40 %; an identical tree once timed +30 % slower under load.

## Known hot spots / lessons
- **Audio thread vs game thread:** the game thread waits for the runtime lock while a block renders. Keep real-time
  paths lock-free, allocation-free and log-free.
- **Logging in hot paths** (per-post `info!`, per-frame trace lines) costs real time; gate it, and write logs from a
  background thread.
- **Bevy/rodio: never `PlaybackSettings::LOOP` an endless custom source:** LOOP = `repeat_infinite()` ->
  `Buffered`, hundreds of ms of audio per device callback under the runtime lock. `ONCE` plays it forever with one
  block per pull.
- **Synchronous file I/O + decode on the game thread** (bank loads on first approach: multi-ms frames). Fixed by a
  distance prefetch. Pattern for moving I/O off the game thread:
  - Move only the pure part (read + parse + decode); the state change stays at its old call site. Identity then
    reduces to "same bytes in" plus "the new per-frame code touches no runtime / RNG state".
  - Never delay the consumer: the slot is done (take), running (wait, never longer than doing it here) or queued /
    failed (load here as before).
  - Size the memory first, and count where the cost moved (freeing big buffers belongs on the worker too).
  - Prove "no work on this thread" with a `cfg(test)` thread-local counter in the decode helper.
  - Identity tests for the move: a Debug-equal bank and `to_bits`-equal PCM for all 66 emitter banks, plus the
    evaluator RNG and block count before / after the new per-frame code.
  - The first prefetch version freed evicted banks on the game thread (0.5 ms calls): hand big drops to the worker.
  - Memory: decoded emitter banks per district map are 160-250 MiB (too much to hold), a distance ring over a real
    session adds 20-30 MiB at 60 / 90 m look-ahead.
  - The e2e bench does not cover host-side systems (no world emitters in the harness): pair e2e identity with
    data-gated unit tests on a real install, and build both e2e runs from one snapshot of the tree when other work
    edits the working tree.
- **Setup speed:** measure and prove with skill `setup-bench` (timed full setup into a scratch install + a sha256
  manifest, compared against a second run of the unchanged pipeline: about 40 files vary run to run). Knobs:
  `setup_budget.py` (`SKATE_SETUP_MAP_WORKERS`, `SKATE_SETUP_THREADS`, `SKATE_SETUP_PRIORITY`, one 75 % core budget
  `setup_budget.cores`). The export pass went from 228.3 s to 135.3 s with identical output: parallel vgmstream
  decodes (manifest filled in order), native RefPack in the vendored UI tools, a block-at-once NumPy DXT decoder (the
  old one kept as `_decode_dxt_reference` with a 480-case test), the HUD export and the living world in their own
  processes, map jobs started right after core, threaded archive extraction and receipt hashing.
  Lessons: threads only pay where the work releases the GIL (zlib, file I/O); in-process Python stages gain nothing
  from threads, prefetching npz arrays on threads was slower, and two threads in NumPy crashed Python (BLAS). Overlap
  stages as separate processes (`stage_job.py`). Profile a stage with `python -m cProfile -s cumtime <script>` on the
  snapshot. Check what a moved stage reads and writes before overlapping it (grep the other stages for its output
  folders). A change to a fingerprinted setup script makes every user's refresh redo that group.
- **Recomp research hooks** on an audio render thread can delay the game's command-queue drain until it overflows:
  keep hooks light (skill `recomp-research`).
