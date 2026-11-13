---
name: make-mod
description: Create or modify a Lua mod (API 2) for skate3rust - physics bodies, graphics, vehicles, minigames, audio. Use when asked to make, fix, or validate a mod.
---

# Make a skate3rust mod

1. Read `sdk/AGENTS.md` first: it is the authoritative workflow. Then `sdk/skate.lua` (API annotations) and the
   relevant `sdk/*_API.md`.
2. Start from the closest example in `sdk/examples/` (physics-sandbox, game-of-skate, simon-says, skyline,
   broken-bones, wipeout-challenge, ...) or `mods/Skyline_Drive_Mod/`.
3. `mod.json` must have `"api": 2`, an `id`, `name`, `version`, `entry` (usually `main.lua`), optional `settings`,
   optional `"enabled_by_default": false` (dev / test mods: off until enabled in the mod menu or by
   `SKATE3_MODS_ENABLE=<id>`; a saved preference wins). The entry file returns a callback table.
4. No host vehicle API exists: build vehicles from primitives (bodies, joints/motors, sensors) in Lua.
5. Validate: `cargo run --locked -p skate-mods --example check_mod -- <mod-folder>`.
6. Install by placing the folder or `.zip` under `mods/`.

Tips: `on_update(event)` / `on_ui_update(event)` / `on_fixed_update(event)` get `event.dt` (seconds); count it down
for timed HUD text. `sdk.ui.text(id, "")` clears a text line; `sdk.ui.remove(id)` removes the element.

Engine side, if the API itself must change: `crates/skate-game/src/modding/`, runtime `crates/skate-mods/`,
dynamics `crates/skate-dynamics/`. Every engine feature should be reachable from a mod: data-driven values (retail
values as defaults from setup data), stable identities a mod can override, a mod-facing entry point next to the
engine-facing one, and cleanup when the mod is disabled.
Note: `.glb` files are gitignored except explicitly whitelisted ones; add a `!` exception to `.gitignore` if a mod
asset should be committed (never retail-derived assets).

## Audio and world-audio APIs (upstream PR #32, merged; #36, #43 and #44 were folded into it)
Check `sdk/skate.lua` and the audio-modding guide (`docs/hails-additions/16-audio-modding.md`) for the exact capability levels;
the summary below is the shape of the API.

- **World audio** (extension `world_audio`): `sdk.world_audio.spawn(key, 'traffic'|'ped'|'skater', opts)` /
  `update` / `event(key, 'horn'|'alarm'|'speech', opts)` / `remove` / `read(key)` / `info()` publish objects to the
  game's retail world audio (the same components engine systems use). Per-mod and total object caps apply; objects
  are parked after 0.5 s without an update and removed on disable. Retail decides who is audible
  (`read(key).audible`). Spawns that can fail (for example from a missing file) go through `sdk.commands.request` so
  a missing file doesn't fail the mod.
- **Content (no Lua):** `audio.json` at the mod root (capability `audio_content`): `replace` / `add` retail audio by
  identity (sample slots, banks, Splice trees, grain members, wheel streams, ambience beds, emitter records, location
  sets, zones, crossfades, speech takes, `location_programs`, `crossfade_layouts`), `tuning` field merges,
  `maps.<stem>` (custom-map audio). A crossfade bank's layout comes from its `c_main_ambience_crossfade` program or
  `add.crossfade_layouts` (a WAV-only mod bank; wins over a program); with neither it is silent. Speech clip names
  must parse (`<event>_<voice>[_<voice name>]_<line>`); unknown clips are merge warnings. First mod by id wins per
  identity; conflicts show in the mod menu. WAVs PCM16 1-2 ch 8-48 kHz, length limits apply (beds longer).
- **Custom maps:** `<map>.audio.json` next to the `.skate`, or a `.skate` `AUDO` extension (schema 1 JSON):
  district, ems, emitters / reverb zones, box regions per layer, crossfade bank.
- **Runtime (capability `audio`):** `sdk.audio.post(key, class, words)` / `redeliver` / `release`,
  `set_global(name, value|nil)`, `watch{globals, mixmap}`, `handle` / `global` / `mixmap` / `info`,
  `sdk.engine.inspect(key, 'audio_catalog')`. Posts apply at the next audio pass; handles die at map change /
  restart. Later levels add `sdk.audio.play{native=true, falloff={radius,core,curve}, reverb, group}` (a WAV through
  the native mixer), `sdk.audio.set_tuning(domain, patch|nil)` / `tuning(...)` / `tuned()` (domains player / world /
  bus / reverb; strict field merge), `sdk.audio.set_mixmap_input`, `sdk.audio.seed`, and world `emitter` /
  `reverb_zone` spawns.
- **Events (capability `audio_events`):** `sdk.audio.subscribe{tags={...}}`, `sdk.audio.events()` (pop, land,
  grind_start, grind_end, footstep, horn, alarm, emitter, zone_change, speech). Observe only. Rules:
  `sdk.audio.rule(key, {match, action = mute|replace|layer, play})` and `audio.json` `rules`.
- Content changes hot-swap where possible (`sdk.audio.info().swaps / last_change`); `audio.json` edits reload while
  the script runs.
  MixMap and grain changes still need a restart. `audio.json` `add.projects` adds a mod's own `.csi` projects
  (names must not clash with the install's). Commands that can fail (a native play without the native audio, an
  unknown tuning field, an unknown emitter bank or reverb preset, a missing rule WAV) go through
  `sdk.commands.request`.
- **Check:** `cargo run -p skate-mods --example check_mod -- <pkg> --install <assets> [--with <pkg>]`.
- Examples: `sdk/examples/audio-example` (ships), `mods/audio-content-test` (dev, off by default;
  `SKATE3_MODS_ENABLE=dev-audio-content-test`), `mods/world-audio-test/` (dev). Synthesize test sounds (for example
  with a small Python script); never copy game audio into a mod.
