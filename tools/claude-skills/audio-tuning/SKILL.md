---
name: audio-tuning
description: Iterate on skate3rust game audio when someone reports a sound that is missing, late, doubled, too loud/soft or wrong - map it to the retail mechanism, measure ours against the recomp (state logs, headless e2e renders, recomp traces), fix it in the native port, and have them listen. Also re-exports audio into the dev install. Use when someone reports how the game sounds or asks to change a sound.
---

# Audio tuning

In upstream `main` (since PR #32 merged) the **native runtime is the only audio path**
(`crates/skate-audio`). Sounds are not "picked": every sound comes from retail's mechanism (AEMS programs, Splice
banks, MixMap, posters), so a wrong sound means a wrong or missing mechanism or input. Port work: skill `aems-port`.
Never commit decoded audio or copy game data into docs. Never build A/B or audition pages; measure internally, people
listen in game. Helper scripts: `tools/` next to this file (installed under `<repo>/.claude/skills/audio-tuning/`;
they find the repo root from there).

## Where things live
- `crates/skate-audio/`: the runtime: evaluator, voice graph, MixMap, grain bed, Splice, buses, player components
  (`player/{contacts,collision,rolling,footsteps,clothing,tricks,treatment,wheels,...}.rs`), world sources.
- `crates/skate-game/src/game_audio/`:
  - `skate_events.rs`: `observe`, the per-frame player audio state from our physics, the state log, `AUDIO_EVENT`
    lines;
  - `native.rs`, `player_audio.rs`, `grain_bed.rs`, `npc_skaters.rs`, `world_sources.rs`: the game-side hosts;
  - `emitters.rs` (the map's `.ems` world emitters), `random_sets.rs` (random distant one-shots with retail's
    scheduler), `ambience.rs` (zone ambience beds and crossfades), `library.rs` (the audio manifest).
- `physics/animation_input.rs` (`AudioEvents` latch).
- Missing setup data logs an `error!` and those sounds stay silent: re-run setup's `audio` group rather than adding
  fallbacks.

## Iteration loop
1. **Write the report down word for word** right after it is made, while it is fresh; ask follow-up questions then.
2. **Get the session.** Play with `SKATE_AUDIO_STATE_LOG=<file>` set (one TSV row per frame). Check it for malformed
   rows first: any malformed row makes the session unusable (record a new one). The game log
   (`logs\game-*.stderr.log`, strip ANSI) has `Game audio: native ...` start lines, `AUDIO_DSP path=`, `AUDIO_EVENT
   ...` and `AUDIO_TIMING` (with `SKATE_AUDIO_TIMING=1`).
3. **Replay it headless** through the e2e harness (`game_audio::e2e`, `E2E_DIR` must be ABSOLUTE, `E2E_FPS=60`) and
   measure per bank / per moment (voices, sum of g^2, posts, tiers) with skill `aems-port`'s `tools/e2e/`. Logged
   inputs replay as logged, so a change to how an input is computed needs a NEW session.
4. **Compare with the recomp** at the same kind of moment (skills `recomp-audio-trace`, `recomp-scripted-runs`).
   Filter to the local rider (`local72`). The recomp is not retail hardware: trust logic and values, not timings.
5. **Find the mechanism** that produces the difference in the recompiled code (skill `recomp-research`) and port it
   (skill `aems-port`): no guessed levels or weightings. Per-frame retail processes run on the console's ~30 fps
   cadence, frame-rate independent.
6. **Prove the rest is unchanged** (e2e bench byte-identical except the intended moments), run tests
   (`cargo test -p skate-audio --locked`, `cargo test -p skate-game --release --bin skate3rust -- game_audio`), build,
   stage, muted smoke run. One game instance at a time.
7. People listen in game; record the verdict word for word and quote it exactly in the PR. Update the docs entry.

## Build, stage, launch
- Never stage while a game runs from `bin\`: `powershell -NoProfile -Command "if (Get-Process skate3rust
  -ErrorAction SilentlyContinue) { exit 3 }"` first. From bash: `powershell.exe -NoProfile -ExecutionPolicy Bypass
  -File scripts/Build.ps1`.
- Smoke runs without noise: `--mute`, a few maps (StartPark, University, DownTown, MegaPark, Industrial): expect per
  map 0 panics / errors / non-finite, render ready, the `Game audio: native ...` lines, `AUDIO_DSP path=fma` on FMA
  CPUs (`tools/regression-checks/smoke_maps.sh` from upstream PR #37 automates this).
- Master volume defaults to 25 %; don't raise defaults.

## Re-export audio into the dev install (after `audio_export.py`/`audio_formats.py` changes)
Game closed first. From the repo root with `py -3.13`:
```python
import io; from pathlib import Path; from tools.asset_pipeline import asset_exports
inst = Path('data/installations') / '<id from data/installation.json>'
asset_exports.audio(Path(r'<extracted disc>'), inst, Path(r'<temp work dir>'), print, io.StringIO(), Path('data/tools'))
```
A real refresh through the installer must still rebuild only the `audio` group (skill `regression-check`).

## World audio data and checks
- **Setup data** (`audio_export.py`): `emitters` (.ems + emitter attributes), `random_sets` (aud_wp_emitters),
  `ambience_zones` (aud_wp_ambiences + crossfades), `regions` (district cSim streams -> region layers); readers in
  `audio_formats.py` (`ems_emitters`, `name_id` lookup8, `region_layers`, `region_key`).
- **Force a location set** for listening: `SKATE_AUDIO_SET=<set name>` (for example `e_dwtn_office_buildings`).
- **Logs:** `World emitters: N of M records`; `AUDIO_EMITTER start|stop`; `AUDIO_RANDOM set|fire`; `AUDIO_AMBIENCE
  zone|crossfade`.
- **Muted in-game test:** `SKATE_REPORT_CHILD=1 SKATE3_MODS=<folder with one test mod>` `bin\skate3rust.exe --assets
  ... --map ...\DownTown.skate --mute`, where the mod teleports through a list of stops (time, position). One mod
  per folder, because every mod in a folder loads.
- **Long-run statistics:** `py -3.13 tools/long_run_stats.py <trace>...`: per stop the retail set, fires per sound
  vs weights, drawn intervals vs the set range, sounds never fired, and the retail ambience zone vs our lookup.
- **Compare with retail:** `py -3.13 tools/check_sets_vs_trace.py <recomp trace> [first=District]`: each stop is
  checked against its own district; stale post-teleport samples are skipped.
- **Bed level:** `py -3.13 tools/check_bed_level.py <session dir> <stop> <bed stream> <zone volume>`: the capture's
  level over a stop vs a prediction from the decoded bed (needs vgmstream from setup).

## Lessons learned
- What sounded "missing" twice turned out to be an unported step, not physics: retail scales the bail region
  impacts by a x1..5 COM-speed curve (`sub_824B0DA8`) and takes |dv.n|. Check the whole chain from physics to poster
  before blaming the physics.
- Animation audio flags persist: push contact and footstep strength stay set for many ticks; brake foot-down
  repeats every tick while braking. Use rising edges, explicit start/stop events and minimum gaps; never "flag set ->
  play".
- `AudibleFootStepStrength` is a loudness level, not a step event: steps come from foot strikes.
- Loops: debounce only while continuously rolling; on re-contact start on the surface actually underneath (a
  remembered surface caused "plays twice").
- A water entry is also a wipeout: suppress the bail when a splash just played.
- A pick by measurement stays "not yet confirmed" until people hear it in game.
