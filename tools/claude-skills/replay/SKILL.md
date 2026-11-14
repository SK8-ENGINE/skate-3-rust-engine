---
name: replay
description: Work on the instant replay and replay editor port - recording every drawn actor, regenerated audio, retail's replay screen and controls (FE 12 / 13), the editor model (clips, speed, shake, look lanes, markers, camera keys), the film pack, the greenscreen and Save to PC video export. Use when changing replay code, answering how retail's replay or editor works, or testing the replay in game.
---

# Instant replay and replay editor

Doc: `docs/hails-additions/37-instant-replay.md` (one section per change, with the retail addresses it ports). Part of
upstream PR #52.

## Code map
| what | where |
|---|---|
| per-actor key tracks, retail i16 quaternions, limits (5400 frames; the byte budget is ours) | `crates/skate-core/src/replay_buffer.rs` |
| editor model: clips, lanes (speed / shake / look / tint), markers (cap 100, spacing 0.0089), encoded length, ScrubFilter, look_at | `crates/skate-core/src/replay_edit.rs` |
| editor modes (retail +0x30 values) and key bindings (FE key indices) as data | `crates/skate-core/src/replay_edit/session.rs` |
| camera key edit rates and the Tripod orbit (`CameraRules`) | `crates/skate-game/src/replay/camera_key.rs` |
| player ring, controls (retail instant mode), audio feed, quit popup, `world_live`, markers, mod limits | `crates/skate-game/src/replay.rs` |
| actor tagging, recording after the presenters, playback, ghosts (`retire`), held hand props, Replay Mode view filters | `crates/skate-game/src/replay/world.rs` |
| audio / fade component tracks; one-shot message tracks | `replay/components.rs`, `replay/messages.rs` |
| the retail replay screen and help on the APT stage | `replay_menu_host.rs`, `retail_menu_stage.rs` (skill retail-menus) |
| editor timeline and Pro Edit Menu | `replay/editor.rs` |
| Save to PC export (Media Foundation loaded at run time) | `replay/export_run.rs` |
| mod API `sdk.replay` (open, marker, set_limits, snapshot.replay_state) | `crates/skate-mods/src/api.lua`, `vm.rs`, `modding/mod.rs` |

## Rules learned
- Record OUTPUTS (drawn poses, audio state, messages), never re-simulate (NPC threads, ped RNGs and the prop solver
  would diverge).
- The living world must not step during a replay (`replay::world_live`, `distributive_run_if` on its fixed chains).
- Anything that despawns goes through `crate::replay::retire` so it stays in the replay as a ghost; every ghost made or
  refused logs `REPLAY_GHOST`.
- Popups raised by a key press open on the key's release.
- Retail per-frame processes run at the console's ~30 fps: step them in whole 30 Hz frames (`ScrubFilter`) so any frame
  rate gives the same result.
- Load optional Windows codecs by hand (LoadLibrary): a `raw-dylib` import would stop the exe from starting on Windows
  N editions without the Media Feature Pack.
- Logs: `REPLAY_OPEN span / frames / actors / bytes`, `REPLAY_CLOSE at`, `REPLAY_GHOST`, `REPLAY_HELP`, `REPLAY_EDITOR
  mode a -> b`, `RETAIL_MENUS popup ... (engine:replay.quit)`.

## Test
- `cargo test --locked -j 4 -p skate-core --lib replay_` and `cargo test --locked -j 4 -p skate-game --bin skate3rust
  replay`, then the whole `--bin skate3rust` before a commit.
- In game without a person at the pad: `--verify <png>` with `SKATE_VERIFY_REPLAY=1` and `SKATE_VERIFY_REPLAY_AT`
  (seconds of recording first), plus `SKATE_VERIFY_REPLAY_HELP=1`, `SKATE_VERIFY_REPLAY_PHOTO=1`,
  `SKATE_VERIFY_REPLAY_EDITOR=timeline|effects` or `SKATE_VERIFY_REPLAY_EXPORT=<file.mp4>`.

## Open
See doc 37's Open lines (for example the id 2 playback reader and the DMO state entries). Online instant replay and the
replay upload are not ported.

## Cameras (2026-10-25; doc 37 "The Follow eye frame is upright", "A camera picked on a keyed clip asks, then replaces its section")
- A Follow key's eye rides the recorded gameplay camera made upright (heading only, up = world up):
  `camera_key::upright`, `FollowEye::Camera`; mods pick `camera_full` or `skater` with `sdk.replay.set_camera`.
  Retail re-evaluates it every frame (`82C8A960` -> `82C8B9F8` -> `82C8C470`, object 2, no smoothing; recomp trace).
- A camera pick inside a keyed section asks "Change Camera" (No / Yes) and replaces that section between hard keys
  (`Editor::pick_replaces` / `pick_asks`, `camera_key::convert_key`); `fill_section = false` keeps plain inserts.
- Retail's camera constraint gate (`0x83037414`) is written by the editor mode every frame: the instant view
  constrains every frame, Timeline playback does not.
- `REPLAY_OPEN actors / bytes` count only world actors (peds, NPC skaters, cars, props); the player has its own track.
