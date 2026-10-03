# 14. Published development and research tools

## Problem

The fork's work (map spawns, collision volumes, setup speed-ups, the native audio) was checked with
many small scripts: regression baselines, equivalence proofs, format readers, audio render analysis,
trace analysis. They lived in a local, untracked workspace with paths hard-wired to one machine, so
nobody else could rerun the checks behind the documents in this folder or build on the readers.

## Change

The generally useful scripts are now in `tools/`, one folder per tool, each with a `README.md` (what it
does, inputs, usage, example output, requirements). An index is in [`tools/README.md`](../../tools/README.md).

| Folder | Contents |
|---|---|
| `tools/regression-checks/` | `check_maps.py` (`--validate-maps` + collision triangle counts), `check_spawns.py` (baked spawns), `check_customiser.py` (customiser output snapshot), `smoke_maps.sh` (muted crash smoke test per map). |
| `tools/setup-equivalence/` | `compare_streams.py` (stream loader byte equivalence), `compare_refpack.py` (native RefPack DLL against the Python decoder). |
| `tools/collision-inspect/` | `map_collision.py`, `analyse_spawn.py`, `collision_attrs.py`, `scan_surfaceless.py`, `unsigned_collision.py`. |
| `tools/world-stream-inspect/` | `big_list.py`, `sim_types.py`, `volumes_dump.py`, `arenas.py` (shared arena reader). |
| `tools/audio-file-inspect/` | `aems_survey.py`, `bank_layout_check.py`, `decode_bank_samples.py`, `multichannel_census.py`, `splc_fields.py`, `ems_dump.py`, `grain_survey.py`. |
| `tools/vault-inspect/` | `find_field.py`, `vault_fields.py`, `vault_layout.py`. |
| `tools/audio-e2e/` | `scenarios.py`, `render_diff.py`, `compare.py`, `frame_levels.py`, `voices_summary.py`, `bank_rate.py`, `wet_level.py`. |
| `tools/audio-bench/` | `e2e_bench.sh`, `compare_runs.sh`, `timing_summary.py`, `audio_timing_summary.py`, `emitter_bank_memory.py`, `prefetch_memory_sim.py`. |
| `tools/recomp-trace/` | For the recomp's research hooks: `trace.py`, `first_pass.py`, `resolve_trace.py`, `retail_voices.py`, `retail_relay.py`, `grec_level.py`, `grec_clean.py`, `grec_material.py`, `send_vectors.py`, `grain_trace_stats.py`, `retail_windows.py`, `veh_trace.py`. |
| `tools/recomp-code-search/` | For the recomp's research hooks: `fn.sh`, `grepfn.sh`, `callctx.sh`, `ppcxref.py`. |

What changed against the working copies:
- Hard-coded paths became arguments with defaults relative to the repository: `--disc` (extracted disc
  root, or `$SKATE3_DISC`, default `.local/skate3-disc`), `--assets`, `--vault`, `--work`, `--baseline`,
  `--exe`, and environment variables for the shell scripts (`SKATE_EXE`, `BENCH_DIR`,
  `RECOMP_GENERATED`). Work output goes under `.local/` (gitignored).
- The regression baselines are not shipped. Each check records its own on a known-good install
  (`--update` / `--save`) and stores it in `.local/regression/` by default.
- Game-derived constants became inputs: `retail_windows.py --contact-callers`,
  `grec_material.py --no-contact`, `emitter_bank_memory.py --group`, `vault_layout.py --name`,
  `grain_trace_stats.py --exclude`; `ppcxref.py` takes the image, function list, base and code range as
  arguments.
- `sim_types.py` / `volumes_dump.py` now share a small arena reader (`arenas.py`) instead of depending
  on a local research script.

## Recomp tools

The two `recomp-*` folders only work with traces and sources from the research hooks of the Skate 3
recompilation:
[Hailey-Ross/skate3recomp, branch `research-hooks`](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)
(see [13](13-recomp-research-hooks.md)). Their READMEs, and the index, label them as such and say they
are reference only: users must build the recomp from their own copy of the game and set up the paths
themselves. The recompilation is not a perfect copy of the console game (uncapped frame rate, different
timing and threading), so its traces are evidence for values and logic, not for timings.

## Left out

- Anything that embeds game code or data: disassembly listings, golden scripts generated from the game,
  the MixMap reference evaluator (it carries a name table taken from the game and reads tables at fixed
  image addresses), and the image readers built around fixed address lists (registries, vtables, type
  tables, image constants).
- Tools tied to one local setup: the comparisons against the local audio proof-of-concept worktree,
  the dev-install staging shortcuts (setup now does the same steps), and the upstream-sync helpers for
  this fork's branch workflow.
- One-off investigation scripts (NPC speech, pedestrian chases, census layers, single-question surveys).

## Verification

- Every published Python script compiles (`python -m py_compile`) and prints its usage with `--help`.
- Every shell script passes `bash -n`.
- No private paths, names or tool-specific notes remain in the published files (searched).
- The tools were not rerun against game data while publishing; their logic is unchanged from the
  working copies apart from the argument handling listed above.

## Open questions

- `crates/skate-game/src/game_audio/e2e.rs` still names the old local script paths in its header
  comment; it could point to `tools/audio-e2e/` instead.
- None of this is offered upstream yet; it would make a tooling PR of its own (see the PR plan).

## Credits

[skate3recomp](https://github.com/mchughalex/skate3recomp) by @mchughalex, the
[rexglue SDK](https://github.com/rexglue/rexglue-sdk), [Xenia](https://github.com/xenia-project/xenia)'s
Xbox 360 research, [vgmstream](https://github.com/vgmstream/vgmstream) for audio decoding, and the
vendored map extraction tools in `tools/vendor/`.

## Added 2026-10-03

- `tools/recomp-trace/bail_impacts.py`: per-bail ragdoll impacts and sound posts (BAILSTEP / BAILREG).
- `tools/recomp-trace/collision_posts.py`: collision-sound posts by poster and owner, with the local-rider flag.
- `tools/recomp-trace/trace.py`: knows the newer line kinds (BAIL*, COLLPOST, BANDQ, LOCALTEST).
