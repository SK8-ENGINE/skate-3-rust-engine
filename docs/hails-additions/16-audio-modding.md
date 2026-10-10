# Audio modding

Status: IMPLEMENTED (2026-10-04) for the scope chosen on 2026-10-03: the content overlay (R1), map audio as data (R2),
retail posts / globals / read-only MixMap (R3a), observe-only audio events (R5) and docs / examples (R6), on the audio
modding PR (upstream #36, folded into #32 on 2026-10-04 together with #43), which builds on the rest of #32 (the native audio engine) and doc 15 (the world-object surface).
Speech is included. Without audio mods the game sounds exactly as before: every phase was checked byte for byte
against the headless end-to-end renders (below). The deferred items are built as code on the follow-up branch
`audio/moddability-2` (2026-10-04, sections H–L): mod WAVs through the native mixer (H), tuning writes at run time
(I), mod emitters and reverb zones as world-audio objects (J), declarative mute / replace / layer rules (K); each
proven byte-identical without mods, with its open questions in L. The third pass on that branch (section M) builds
the rest of the "Later" list: content changes hot-swapped without a restart (L1), writable MixMap inputs (L2), own
MixMap instances for world objects (L3), mod Csis projects (L4), a seedable audio random state (L5), `audio.json`
hot reload while the mod runs (L7), offsets in the owner's axes and moving emitters.

The guide (sections A–G, and H–L for the follow-up) says what a mod can do and how. The design and research it was built from follow (sections
0–4, unchanged apart from status notes).

---

## A. What a mod can change, at a glance

| want | how | capability |
|---|---|---|
| replace or add retail sounds (sample slots, whole banks, Splice trees, grain members, wheel streams, ambience beds, speech takes), sets, zones, crossfades, emitter records, tuning fields | `audio.json` (data, no Lua needed) | `audio_content` = 1 |
| give a custom map audio (emitters, reverb zones, zone ambience, location sets, crossfades) | `<map>.audio.json` next to the `.skate`, a `.skate` `AUDO` extension, or a mod's `maps` section | `audio_content` = 1 |
| play a retail sound (post to a retail class), set a retail global, read MixMap outputs | `sdk.audio.post / redeliver / release / set_global / watch / handle / global / mixmap / info` | `audio` = 2 |
| react to the game's sounds (pop, land, grind, horn, …) | `sdk.audio.subscribe / events` | `audio_events` = 1 |
| play the mod's own WAVs (with `audio` = 3 through the game's mixer by default; `native = false`: outside it, as before) | `sdk.audio.preload / play / update / stop / stop_all` | `audio` ≥ 1 |
| play the mod's own WAVs through the game's mixer (reverb, retail pan and distance law) | `sdk.audio.play{…, falloff = …}`, the default (H) | `audio` = 3 |
| change tuning while the mod runs (player / world / bus / reverb presets) | `sdk.audio.set_tuning / tuning / tuned` (I) | `audio_tuning` = 1 |
| publish cars, peds, skaters to the world audio | `sdk.world_audio.*` (doc 15) | `world_audio` = 1 |
| add sound emitters and reverb zones | `sdk.world_audio.spawn(key, 'emitter' / 'reverb_zone', …)` (J) | `world_audio` = 2 |
| mute, replace or layer the game's own sounds (the replacement at the owner, or where the mod says) | `sdk.audio.rule(key, rule)` or `audio.json` `rules` (K) | `audio_events` = 2, `audio_content` = 2 |
| a rule sound's offset in the owner's own axes; follow a moving published emitter | `play.frame = 'owner'` (M7) | `audio_events` = 3 |
| turn an audio mod on / off or edit its `audio.json` and files while it runs without cutting the sound | automatic: hot swap and reload (M1, M6) | `audio_content` = 3 |
| add new Csis classes, functions, globals | `audio.json` `add.projects` (M4) | `audio_content` = 3 |
| drive retail's MixMap controllers (a duck through the Master gains, a flag) | `sdk.audio.set_mixmap_input` (M2) | `audio` = 4 |
| reproducible audio draws for tests | `sdk.audio.seed(n)`, `SKATE_AUDIO_SEED` (M5) | `audio` = 4 |
| a mod car / ped heard beside retail's pools | the default: `sdk.world_audio.spawn(key, 'traffic' / 'ped', …)` (M3; on 3: `{slots = 'own'}`) | `world_audio` = 4 |
| a mod car / ped in retail's pools with the map's objects (the nearest win) | `sdk.world_audio.spawn(…, {slots = 'shared'})` (M3; alias `retail`, also on 3, where it is the default) | `world_audio` = 4 |

Test `(sdk.capabilities.audio or 0) >= 2` (and so on) before relying on a feature; an older engine lacks the keys.

## B. The content overlay: `audio.json`

A mod ships `audio.json` at its root, next to `mod.json`; files are mod-relative paths (`/`-separated, no `..`).
It applies **only while the mod runs**: when the mod starts, stops, is reloaded or its script fails, the new set of
overlays is swapped into the running audio (M1: only what changed is replaced; a MixMap or grain replacement still
restarts the game's sound once, a short cut). Editing `audio.json` or a file it names while the mod runs reloads the
content without restarting the script (M6). Overlays merge in mod-id order over the
install, by retail identity; **the first mod to change an identity wins** and later claims are listed in the mod menu
("Audio conflict") and the log. An entry the install does not have (an unknown bank, slot, set…) is skipped with a
warning, never an error. A whole overlay that fails its checks is not applied, the mod still runs, and the menu says
why.

```jsonc
{
  "version": 1,
  "replace": {
    "samples":   { "Skate_Collisions": { "3": "audio/pop.wav", "4": { "file": "audio/loop.wav", "loop_start": 1200 } } },
    "banks":     { "C04_taxi01": { "abk": "audio/C04_taxi01.abk", "samples": ["audio/t0.wav"], "group": "world" } },
    "splice":    { "Skate_Collisions": "audio/Skate_Collisions.splc" },
    "grains":    { "wood_ramp_hard": { "file": "audio/wood.wav", "grain": "audio/wood.grain" } },
    "wheels":    { "Whls_spins_Jump_1": "audio/spin.wav" },
    "ambience":  { "04_dt_main": "audio/dt_main.wav" },
    "mixmap":    "audio/MixMapSK8.mxb",
    "emitters":  { "sfx_downtown": { "9": { "position": [0, 0, 0], "extent": [18, 18, 18], "bank": "Baby_Cry_1", "patch": 22 } } },
    "random_sets": { "e_dwtn_spillway_brewery": { "sounds": [ { "bank": "Siren_city_8", "weight": 3 } ] } },
    "zones":     { "int_tunnel": { "bed": "22_interior_tunnel_amb", "volume": 0.5 } },
    "crossfades": [ { "from": "1F741D87EB58E84F", "to": "B6CE4BDEB63B6639", "group": 2 } ],
    "speech":    { "livingworld": { "501_41_adtm1_Warn_n.dat": { "0": "audio/warn.wav" } } }
  },
  "add": {
    "banks":     { "MOD_siren": { "samples": ["audio/siren.wav"], "preload": false, "group": "world" } },
    "emitters":  { "sfx_mymap": [ { "position": [1, 2, 3], "extent": [6, 6, 6], "bank": "MOD_siren" },
                                  { "position": [1, 2, 3], "extent": [12, 6, 12], "kind": 5, "reverb": "BEEFC8E3DE04FBAE" } ] },
    "random_sets": { "00000000000DE501": { "name": "my_sirens", "sounds": [ { "bank": "MOD_siren" } ], "min_interval": 5, "max_interval": 10 } },
    "zones":     { "00000000000DE502": { "name": "my_zone", "bed": "04_dt_main" } },
    "crossfades": [ { "from": "00000000000DE502", "to": "DB21C7A69325F3DF", "group": 1 } ],
    "speech":    { "livingworld": { "501_41_adtm1_Warn_n": ["audio/warn_extra.wav"] } },
    "location_programs": { "MOD_siren": [ { "sample": "shuffle", "level": 0.8, "pan_sweep": 30 } ] },
    "crossfade_layouts": { "MOD_fade": { "1": [ { "sample": 0, "pan": 45 }, { "sample": 0, "pan": 225, "level": 0.7 } ] } }
  },
  "tuning": { "world": { "traffic_engine": { "c04_taxi01": { "idle_rpm": 1200 } } } },
  "maps": { "MyMap": { "district": "DownTown", "ems": ["sfx_mymap"],
                       "regions": { "audio_emitters": [ { "box": [0, 0, 50, 50], "key": "my_sirens" } ] } } }
}
```

Identities and what each section does:

| section | identity | notes |
|---|---|---|
| `replace.samples` | bank stem + S10A slot | an AEMS bank's slot gets a sample header built from the WAV (rate, length, channels); a slot that looped loops from frame 0 unless `loop_start` is given. Programs with fixed timers may cut a longer replacement. Sample banks played by Bevy voices (location sets, crossfades) and Splice banks (pops, landings, foley: Splice builds headers from the PCM) take it as is. |
| `replace.banks` / `add.banks` | bank stem | the program (`abk`, optional for a replace) and / or the WAVs by slot. A new AEMS bank binds to the retail classes its exports name (e.g. a new engine bank bound to `TRAFFIC_CAR` with its own patch). `preload: true` loads it when the audio starts and keeps it across map changes; `group` = `player` (Effects volume) or `world` (Ambience volume). |
| `replace.splice` | bank stem | the whole Splice patch tree (expert). |
| `replace.grains` | grain member | the whole recording and its `.grain` member. |
| `replace.wheels` / `replace.ambience` | stream / bed name | beds may be up to 600 s. |
| `replace.mixmap` | the MixMap | the whole `.mxb` (expert). |
| `replace.emitters` / `add.emitters` | `.ems` file + record index | sound emitters (`kind` 1, a bank and patch) and reverb zones (`kind` 5, a reverb preset key). Added records join a file (a new file name makes a new file a map can list). |
| `replace.random_sets` / `zones` | 16-hex key or name | the whole record; a replacement keeps the record's name unless it gives one. `add` takes a new 16-hex key. |
| `replace.crossfades` / `add.crossfades` | zone pair (either order) | |
| `replace.speech` / `add.speech` | archive (`livingworld` = peds and NPC skaters, `maincast` = the pros and the special cast) + clip (with or without `.dat`) + take | added takes join the clip after its own, so the speech manager can pick them. Clip names must read as speech clip names (`<event>_<voice>[_<voice name>]_<line>`, an error otherwise); when the overlays merge (game and `check_mod --install`) each clip is looked up in the install's speech index: an unknown clip (with a "did you mean" hint), an archive the install has no index for, or a replaced take past the clip's own is a warning and is not applied. |
| `add.location_programs` | bank | the layers a location-set post of the bank plays (the interim Bevy-voice player); a mod bank without a row plays one shuffle layer at level 1, a retail bank without one stays silent (as retail). |
| `add.crossfade_layouts` | crossfade bank | group (1..64) → the looping voices the zone-transition crossfade plays: sample slot, `pan` in degrees (0 = ahead, 90 = right), `level` 0..1; 1..8 voices. For a crossfade bank without a program (a bank of WAVs). A bank with a `c_main_ambience_crossfade` program (retail's three, or a mod's `.abk`) gets its layout from the program, as retail does; a declared layout wins over a program. The bank's samples must exist (warning otherwise). |
| `tuning.player / world / bus / grain` | section + field path | field merges onto the install's tuning sections (e.g. `world.ped_objects` body-fall ids / tazer time, `world.speech_voice` peak filter / echo delay, `world.speech_tuning`); only fields the install has; arrays take decimal index keys; numbers replace numbers. An overlay whose merged tuning does not read back is left out whole. |
| `maps.<stem>` | map stem + field | see C. |

Files are checked when the mod starts (and by `check_mod`): WAVs must be PCM16, 1–2 channels, 8–48 kHz, at most 30 s
(beds 600 s, wheel / grain streams 60 s); `.abk`, `.splc`, `.mxb` and `.grain` files must parse. Limits: 64 MiB of PCM
per mod and 256 MiB for all running audio mods, 512 sample replacements, 64 banks, 1024 files and 4096 records per
mod. Licence note: overlays reference retail identities (names, keys, slot numbers); mods ship their own audio.

## C. Map audio (custom maps)

A map's audio is assembled when the map loads, in this order (later sources override the fields they set and append
their records and boxes):

1. **retail**: the map's database entry, read at run time from the stock collections every install has (the `world`
   row whose `WorldStream` is `DIST_<stem>` → its map entry: the `.ems` list and the crossfade bank). No setup refresh.
2. **the map's own definition**: the `.skate` `AUDO` extension (schema 1, UTF-8 JSON), else a `<map>.audio.json` file
   next to the `.skate`.
3. **mods**: `maps.<stem>` in `audio.json` (first mod by id per field).

All three use the same shape:

```json
{
  "district": "DownTown",
  "ems": ["sfx_downtown"],
  "crossfade_bank": "Main_Ambience_Crossfade_DT",
  "fallback_bed": "04_dt_main",
  "emitters": [ { "position": [0, 0, 0], "extent": [6, 6, 6], "bank": "Baby_Cry_1", "patch": 22 },
                { "position": [40, 0, 10], "extent": [20, 8, 20], "kind": 5, "reverb": "BEEFC8E3DE04FBAE" } ],
  "regions": {
    "audio_ambience": [ { "box": [0, 0, 64, 64], "key": "int_tunnel" } ],
    "audio_emitters": [ { "box": [0, 0, 128, 128], "key": "e_dwtn_spillway_brewery" } ],
    "audio_reverb":   [ { "box": [0, 0, 32, 32], "key": "BEEFC8E3DE04FBAE" } ]
  }
}
```

- `district`: whose retail world-painter regions apply (zone ambience, location sets, reverb). Default: the map's stem.
- `emitters`: extra `.ems`-style records after the files' (sound emitters and reverb zones, reached and released as
  retail's).
- `regions`: axis-aligned boxes `[centre x, centre z, half x, half z]` per layer, checked before the district's tiles.
  `key` is a 16-hex key or a zone / location-set name (reverb boxes take a preset key).
- `fallback_bed`: a bed for installs without zone data (not retail).
- `crossfade_bank`: the bank the zone-transition crossfade plays (retail: `Main_Ambience_Crossfade_DT / _Ind / _Uni`).
  Which voices each zone pair's group plays comes from the bank itself: its `c_main_ambience_crossfade` program
  (posted per group, as retail's ambience manager does) or, for a mod bank of WAVs, the overlay's
  `add.crossfade_layouts`. A bank with neither stays silent at transitions (logged; `check_mod --install` warns).

Mod files are not referenced from a map definition: a custom map uses retail identities, and new sounds come from an
audio mod (`add.banks`, `add.random_sets`, …).

## D. Runtime API (Lua and Rust)

`game_audio::mod_audio::AudioApi` is the same API for engine systems and mods; the mod commands call it.

```lua
sdk.audio.post('fountain', 'c_emitter', {32767, 30000, 0, 0, 4096, 25000, 0, 0, 22})
sdk.audio.redeliver('fountain', {32767, 20000, 0, 0, 4096, 25000, 0, 0, 22})
sdk.audio.release('fountain')
sdk.audio.set_global('babycry_1_sel_snd', 1)     -- nil restores
sdk.audio.watch{globals={'babycry_1_sel_snd'}, mixmap={{slot='emitter', object=0, instance=0, output=4}}}
local h = sdk.audio.handle('fountain')           -- {live=true, class='c_emitter'}
local v = sdk.audio.global('babycry_1_sel_snd')
local o = sdk.audio.mixmap('emitter', 0, 0, 4)   -- {level=…, raw=…, pitch=…, half=…}
local info = sdk.audio.info()                    -- native, generation, restarts, overlays, conflicts, map, limits, tags
sdk.engine.inspect('catalog', 'audio_catalog')   -- classes, functions, globals (values), loaded banks, map audio, sets, zones
```

- Posts are queued and applied at one fixed point, the start of the next audio pass (before the local player's
  process), so their order against the game's own posts never changes. A key's post replaces its last one. An unknown
  class or global is a command error (use `sdk.commands.request` to receive it as a result instead of failing the
  mod). `c_emitter`'s words are expert-level: the game's own emitters derive them from the MixMap (dry, send, pan,
  pitch, low-pass, …, patch).
- A handle dies at a map change and when the game's sound restarts for an audio content change (a hot swap, M1, keeps it): it reads `live =
  false` and the mod posts again (on `world_changed`). Dead ids are never released into the new runtime.
- Globals: the first mod to set one owns it; `nil`, the mod stopping and a map change restore the value seen before
  its first write; after a restart the override is applied again.
- Limits: 32 handles per mod, 128 in all, 16 posts per frame per mod, 32 payload words, 16 watched globals and 16
  watched MixMap outputs.
- Parity: a post runs the bank's program, which draws from the evaluator's one random generator as every retail post
  does, so with a mod posting the retail random sequence differs from a run without it. Without mod posts nothing
  changes.

## E. Audio events (observe only)

```lua
sdk.audio.subscribe{tags = {'pop', 'land'}}     -- {tags = {}} = every row; subscribe(nil) stops
for _, e in ipairs(sdk.audio.events()) do        -- last frame's rows, each frame once
  if e.tag == 'pop' then ... end                 -- e.kind, e.source, e.class, e.slot, e.id, e.owner, e.tag
end
```

Rows come from the post sites themselves: the local player's component posts / releases and Splice starts, the world
and NPC hosts (posts, releases, ped Splice steps, ped body falls and phone rings, the ped tazer, the NPC loose-board slide),
world emitter start / stop, zone ambience changes and speech line starts (class `speech` for the living world, `maincast`
for the main cast). Tags: `pop`, `land` (the board contacts' Splice starts with the running player's Contacts tuning pop and landing ids, whenever the mod subscribed),
`grind_start`, `grind_end` (the grind slot's post / release), `footstep`, `horn`, `alarm`, `tazer` (a ped's `c_tazer`
post), `body_fall` (a ped's body-fall Splice start), `emitter`, `zone_change`, `speech`, `body_impact` (a body region's hit of the local player or an NPC skater from the body poster `sub_824BC188`; the row adds `region`, `impact`, `tier`, `material`, `position`; observe only, not a rule tag). Rows are one frame late; at most 256 a frame (`truncated` says when more happened). Nothing is recorded while
no mod subscribes. Muting, replacing or layering a retail sound: declarative rules (K).

## F. Tooling, lifecycle and the menu

- `cargo run -p skate-mods --example check_mod -- <package>` checks `mod.json`, the Lua syntax and `audio.json` in
  depth and prints a summary; `--install <assets folder>` also merges it over the install as the game does and lists
  unknown identities (speech clips and takes are checked against the install's speech indexes), says where each
  crossfade bank the overlay names or lays out gets its layout (program / declared, or a warning when it has none),
  and `--with <package>` adds other audio mods to report conflicts.
- The game's native audio starts after the mod manager's first scan, so an audio mod enabled at boot costs no second
  start.
- The mod menu shows, for the selected mod, its `audio.json` load error, conflicts (the first by mod id wins) and
  skipped entries; with no mod selected, a count of audio problems.
- Example: `sdk/examples/audio-example` (self-made chimes as a location set on the format-demo map, a click layered on
  pops from the events). A larger dev test mod (off by default) is `mods/audio-content-test`.

## G. Implementation (for the upstream PR)

Problem: before this PR a mod could only play its own WAVs outside the native mixer and publish world objects; no
retail content could be replaced or added, custom maps had no emitters / zones / sets, and nothing could post a
retail sound or observe one. Root cause: retail audio data reached the engine through one install-only manifest
reader, the map → `.ems` lists and crossfade banks were code tables for the ten retail maps, and no runtime entry
existed outside `game_audio`.

Changes:
- `crates/skate-mods/src/audio_content.rs`: the `audio.json` schema (`deny_unknown_fields`), checks, per-kind WAV
  limits, deep file checks through the skate-audio parsers (skate-mods now depends on skate-audio, pure Rust).
  `audio_merge.rs`: the pure JSON merge by identity (first owner wins, conflicts, warnings); `check_mod`.
- `crates/skate-game/src/game_audio/library.rs`: `Library::load_with` (mod files as `mod:<id>/<path>`, per-file bank
  sources, sample headers rebuilt for mod WAVs in AEMS banks, speech take overrides, location programs, resident
  banks). With no overlays it reads the manifest exactly as before.
- `content.rs`: `AudioContent` (the running set, load errors, the merge report, the content generation), the native
  start after the first scan, the restart (new runtime continuing the old post ids with `map_epoch` + 1, fresh world /
  NPC / speech hosts: old handles forgotten, never released; stream respawned; the prefetch worker ends with the old
  runtime). Emitters, reverb zones, zone ambience and location sets key on the content generation.
- `map_audio.rs`: `MapAudio` (retail lookup from the stock collections, `AUDO` tag / sidecar, mod sections, box
  regions); `skate-data` `Field.array`; `CurrentMap.audio_tag`. The old code tables remain only as the test oracle.
- `mod_audio.rs`: `AudioApi` (posts, globals, watches, catalog, info) and the events (an observing Splice host
  wrapper, per-site buffers that exist only while a mod subscribes, a double buffer at the start of the pass).
- `skate-audio`: `Evaluator::next_node / continue_nodes`. `skate-mods` `vm.rs` / `api.lua`: the new commands and
  wrappers, capabilities `audio` = 2, `audio_content` = 1, `audio_events` = 1. Mod menu lines; `sdk/skate.lua`.

Verification:
- No-mod identity: the headless e2e bench (13 scripted scenarios and whole play sessions, the 60 Hz host and 300 fps;
  84 renders and voice logs) is byte-identical to the pre-PR build after every phase (R1, R2, R3, R5); the merge of the
  #32 work into this branch did not change it either, nor did the second merge (#32 at `cb71483` with
  optimisation pass 2 and the world gaps — tazer, ped body falls, speech echo, main-cast speech, NPC loose-board
  slide — plus upstream `main` `4488651`): the 84 outputs equal #32's own bench run of that tree. A data-gated test shows the merge path with an empty overlay
  reproduces the real install's manifest field for field.
- R1: a restart mid-scenario plays bit for bit like a fresh runtime from that point; a mod replacing an emitter bank's
  samples changes the output and removing it restores retail exactly; preloaded mod banks survive map changes;
  replaced AEMS samples get their own headers; Splice / grain / speech / set / zone / emitter overlays reach their
  readers; conflicts and rejected overlays are reported.
- R2: the run-time retail lookup reproduces the old tables for all ten maps (files, order, crossfade banks) and the
  reverb-zone records keep their ids; sidecar, tag and mod sections stack as described.
- R3: post limits, dead handles (map change, restart), global restore, watches; a mod's `c_emitter` post plays.
- R5: tags fire on real posts (pop, land, grind start / end through the real banks; horn, alarm, ped footsteps through
  the world host); nothing is recorded without a subscriber; frame timings equal within noise.
- World gaps (after the second merge): main-cast takes, the ped one-shot tuning, the speech echo delay and the
  `Tazer` bank's samples come from an overlay (`world_gaps_data_comes_from_the_overlay`, data-gated); ped ring /
  body-fall / foot-plant Splice starts, the tazer post (tag `tazer`, slot `ped_tazer`), body falls (tag `body_fall`)
  and main-cast line starts (speech rows with class `maincast`) are recorded for subscribers
  (`world_event_tags_fire_on_real_posts` sees `tazer` and both `body_fall` starts of a tazing, falling ped).
- Pre-existing failures unrelated to this PR: `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`
  (listed in PULL-REQUESTS.md).

Open questions / later: a bank-level hot swap instead of the restart (done: M1). (Crossfade layouts for mod crossfade banks
are closed below; tuning writes at run time, mod emitters and reverb zones, mod WAVs through the native mixer and
the mute / replace / layer rules are built on the follow-up branch: H–L.)

### Closed after review (2026-10-04)

Gaps the first version left open:

- **Mod crossfade banks played no crossfade.** Problem: a mod (or custom map) could name its own crossfade bank, but
  the interim player looked the zone pair's group up in a measured table that knew only retail's three banks, so a mod
  bank stayed silent at zone transitions. Root cause: the layout (which samples, from which directions, at which
  level) was engine data, while in retail it is the bank's own data: the ambience manager posts
  `c_main_ambience_crossfade` with the pair's group in w9 and the bank's program opens the voices. Change:
  `skate_audio::world::crossfade::layouts` posts every group (1..25) at full level to a private evaluator and records
  the voices the program opens (slot, azimuth input, level input); `game_audio/crossfade_layouts.rs` builds a map's
  layouts when its crossfade bank or the audio content changes (declared `add.crossfade_layouts` first, then the
  program, else none + a warning) and `ambience.rs` plays them; the table is now only the test oracle
  (`crossfade_groups.rs`, `#[cfg(test)]`). Program layouts are rounded to the table's precision (pan 0.1°, level
  1e-4), so retail plays exactly as before. Evidence: `retail_layouts_come_from_the_banks_programs` (data-gated): all
  21 groups of the three retail banks, voices in the same order, equal to the table bit for bit; deriving all three
  banks takes ~8 ms (about 3 ms at a map load). Mod tests: `a_mod_crossfade_bank_gets_its_layout` (data-gated:
  declared layout, a program shipped under a mod bank name with mod WAVs, a bank with neither) and
  `a_mod_crossfade_bank_plays_its_layout` (the ambience system walks from one zone into another: the declared voices
  play the mod's samples for the transition and stop when the new bed has faded in). The dev test mod now makes a
  self-made bank (`DEV_fade`) DownTown's crossfade bank.
- **Speech clip names were only checked when the speech index loaded** (a log warning at run time, after the merge
  had accepted them). Change: `validate` rejects names that do not parse as speech clip names and unknown archives;
  `audio_merge::merge_one_with` / `merge_with` take the install's speech indexes (`SpeechClips::load`, read only when
  an overlay names speech) and turn unknown clips (with a "did you mean" hint), missing archives and takes past a
  clip's own into warnings that skip the entry, in the game (mod menu, log) and in `check_mod --install`. Tests:
  `speech_takes_are_checked_against_the_speech_index` (skate-mods), `speech_takes_are_checked_when_the_overlays_merge`
  (game library), the validation cases in `bad_overlays_are_rejected`.
- **Found by the in-game check: empty Lua tables broke audio commands.** An empty Lua table has no array part and
  reaches serde as an empty map, so `sdk.audio.watch{mixmap = …}` (whose wrapper sends `globals = {}`),
  `sdk.audio.subscribe{tags = {}}` and `sdk.audio.post(key, class, {})` failed with "invalid type: map, expected a
  sequence"; the dev test mod's `on_load` died on it, so its overlay never applied (as designed: overlays apply only
  while the mod runs). Unit tests had sent JSON arrays and non-empty Lua tables. Root cause: `Command` is an
  internally tagged enum, so serde buffers the whole table first (mlua's own `deserialize_seq` would accept `{}`, the
  buffered map does not). The same issue was in every other list field a script can fill, audio or not. Change: one
  shared helper, `crates/skate-mods/src/lua_list.rs` (`list` / `opt_list`): a sequence reads as before, a table whose
  keys are exactly 1..n (the empty table included) reads as that list in key order, any other table is an error
  ("expected a list, found a table with the key …"); `nil` on an optional list stays "absent". Used on
  `audio_post` / `audio_redeliver` `words`, `audio_watch` `globals` / `mixmap`, `audio_subscribe` `tags`,
  `graphics_mesh` `deform_nodes`, mesh-buffer write / append `positions` / `indices` / `normals` / `colors` / `uvs`,
  `ui_canvas` `items`, `ui_menu` `items` and each item's `children`, `player_detach` `candidates`, raycast `exclude`.
  Fields that are maps or free values (`network_state` `value`, world-audio `value`, struct options such as
  `options = {}`) are not touched. Not changed: `Shape` `points` / `hulls` (in skate-dynamics) still refuse `{}` at
  deserialisation; an empty hull is invalid either way, only the error text differs. Before this, for example
  `sdk.ui.menu(key, {..., items = {{..., children = {}}}})` and `sdk.player.detach{candidates = {}}` failed the same
  way. Tests: `vm::empty_table_lists::every_list_field_takes_an_empty_table_and_a_list` (a real mod through `Vm` /
  `api.lua` / `_submit`: `{}` and a normal list for every field; empty where entries are required = a validation error,
  never a serde one; a named-key table still refused), `map_fields_keep_empty_tables_as_maps`, `lua_list::tests`
  (gaps, 0, negative, fractional and named keys refused; JSON unchanged). The upstream test
  `mesh_buffer_write_rejects_empty_uv_table` now checks the write is refused by validation (it still is, by
  `_submit`) instead of by deserialisation. No-mod e2e `m5` = `m4` (84/84 identical).
- **Found by the in-game autotest: `pop` / `land` were wrong for a mod that subscribed at load.** Problem: a mod
  calling `sdk.audio.subscribe` in `on_load` (the normal case: before native audio starts) got pops untagged for the
  whole session, and any player `Skate_Collisions` Splice row with id 0 tagged `land` (the `land` rows of the DownTown
  log check above were most likely these). Root cause: `AudioApi::subscribe` copied the pop / landing ids from the
  native player only if it was already running; otherwise the tags kept `Tags::default` (no pops, landing 0) until the
  mod re-subscribed. Rules were not affected (they would read the player directly). Change (`mod_audio.rs`):
  `events_frame` copies the running player's ids into the tags every frame a mod subscribes (before it collects that
  frame's rows), so they follow a native start, restart, map change or tuning change whatever the order; no player =
  unset ids. `Tags` holds fixed arrays (`[i32; 6]` pops, the landing id) and is `Copy`: the refresh allocates nothing,
  and with no subscriber `events_frame` returns before it as before. Id 0 (unset) never tags `pop` or `land`. The
  copy at subscribe stays as an early value. Mod API unchanged. Test:
  `event_tags_follow_the_player_whatever_the_subscription_order` (subscribe with no native audio, start it: a pop, a
  hollow pop and a landing tagged, an id-0 row untagged; unsubscribe / re-subscribe; a landing-id change and a
  replaced player retag from the next frame; native stopped → unset). No-mod e2e `b1` = `b0` (this branch's head
  before the fix, itself = `m5`).

Proofs for these changes: the headless e2e bench (84 renders and voice logs) is byte-identical to the previous
reference (`m2`) after them (`m3`, and `m4` with the final tree); all suites pass (skate-audio with ignored, the
game's `game_audio::` tests with ignored and every private-data variable, skate-mods apart from the known missing
Skyline GLB), `check_mod --install` passes the three shipped / dev audio mods with 0 warnings, `cargo build --locked`.

In-game log check (muted, the branch's own staged build, about 60–75 s each, no input):
- DownTown with `audio-content-test`: the overlay merged (0 conflicts, 0 warnings, 0 rejected), the native runtime
  started once (after the first mod scan, no restart), map audio `["retail", "mod"]` with crossfade bank `DEV_fade`,
  `AUDIO_AMBIENCE crossfade layouts DEV_fade: Declared, groups [1..7]`, the spawn walk-in crossed zones and played
  `crossfade DEV_fade group 2 level 1.75`, then `zone dt_main bed 04_dt_main from mod:dev-audio-content-test/audio/bed.wav`;
  the mod logged event rows (`zone_change`, `footstep`, `land`, player posts, `sk8_foley` Splice starts); no errors or
  panics. The replaced `Baby_Cry_1` emitter was not in range of the spawn (unit-tested).
- DownTown without it: map audio `["retail"]`, `crossfade layouts Main_Ambience_Crossfade_DT: Program, groups [1..7]`,
  the same transition played `crossfade Main_Ambience_Crossfade_DT group 2 level 1.75`, the retail bed.
- format-demo with `audio-example`: overlay merged, map audio `["mod"]`, `AUDIO_RANDOM set example_chimes`, a chime
  every 4–8 s (`AUDIO_RANDOM fire EXAMPLE_chime`); without it: map audio `["none"]`, no sets. (Pops need input: the
  pop / land tags are covered by the data-gated tests, and `land` showed in the DownTown run.)

## H. Mod WAVs through the native mixer (the default, capability `audio` = 3)

Status: built on `audio/moddability-2` (2026-10-04, after the first modding cut, then PR #36; all now in #32). **Native is the default** (user decision
2026-10-04, "Yes duh"; it was opt-in until then): every `sdk.audio.play` goes through the game's mixer unless the
sound says `native = false` (the Bevy voice, exactly as before).

| `native` | routing |
|---|---|
| not given (default) | native while the game's native audio runs and one of the 24 native voices is free; otherwise a Bevy voice (no install audio, the native voices all in use), so a mod written for the Bevy path keeps playing |
| `true` | native only; without native audio or a free native voice the play is a command error (`sdk.commands.request` returns it) |
| `false` | always the Bevy voice; the native-only fields (`falloff`, `reverb`, `group`) are refused |

A positional native sound without `falloff` gets the default reach `skate_mods::audio::DEFAULT_REACH`: a 40 m sphere,
no core, the squared curve (retail's audible reach of a traffic car: the vehicle list is cut at 40 m, see doc 15);
before the flip a positional native sound had to give its reach. `spatial_scale` stays a Bevy-only field (ignored by a
native voice). A mod without audio is unaffected.

```lua
sdk.audio.play('siren', {path = 'audio/siren.wav', position = {10, 0, 4}, loop = true,
                         falloff = {radius = 40, core = 0.1, curve = 'squared'}})   -- reverb = true, group = 'world'
sdk.audio.play('ui', {path = 'audio/click.wav', spatial = false, group = 'player', reverb = false})
sdk.audio.play('old', {path = 'audio/old.wav', native = false, spatial_scale = 0.1})  -- the Bevy voice
sdk.audio.update('siren', {volume = 0.5, position = {12, 0, 4}})                    -- as for Bevy voices
sdk.audio.stop('siren', 0.3)
```

Existing mods: the bundled Skyline Drive mod's engine, turbo, brake and pop voices (`body` + `offset`, `spatial_scale`,
no `falloff`) now play natively with the default reach, following the car's body, panned by the retail panner, under
the Ambience volume (`group` default `world`); at most 12 of them at once, inside the 24. The SDK `audio-example`'s
tick (`spatial = false`) plays natively, centred. Their levels differ from the Bevy voices (retail emitter law
instead of Bevy's spatial gain): an in-game listen is open.

What happens:
- The WAV joins the mod's bank in the game's mixer (one bank per mod and volume group; a sample header built from
  the WAV: rate, length, channels; a looping play gets a slot that loops from frame 0).
- The voice is a direct voice on SFX Master, set every frame the way a retail world emitter's `c_emitter` words are
  (mixmap-spec §6.3, `Native::emitter_payload`): dry = MixMap Emitter out4 × level (−6 dB with the global ducks, no
  distance roll-off), environment send = out8 × level (−26 dB rolling off with camera distance 4 → 70 m, the
  reverb route), pan = out0 (the camera azimuth into the retail panner), pitch out5, low-pass out6. The sound's own
  reach is the `.ems` record test of a sphere (`radius`, inner `core`) with the retail falloff curve
  (`eVolumeFalloffType`: `squared` (1 − d)², `linear` 1 − d, `flat`), `level = volume × curve(d)`; outside its reach
  it is silent and keeps playing (a loop is heard again when the listener comes back). A sound without a position
  (`spatial = false`) takes the non-positional outputs (out2 dry, out7 send, out3 low-pass), centred.
- The MixMap instances come from a **private MixMap**: retail's MixMap file built with the Global slot and 32
  Emitter instances, its Global inputs (master, music, reverb, pause) copied from the game's MixMap before each of
  its ticks, ticked with the game's pass (the same console evaluations) and only while an instance is held. Its
  Emitter words equal a retail Emitter instance's for the same position, tick for tick
  (`the_private_mixmap_matches_the_retail_emitter_outputs`, 1,200 word sets over a moving, turning listener and a
  reverb change). The game's own MixMap and its 5 Emitter instances are not touched.
- Volume: `group = 'world'` (default, the Ambience volume, as the world emitters) or `'player'` (the Effects
  volume), under the master volume and `--mute` like every native sound. The stream pauses with the menu / replays.
- Limits: 24 native voices in all; they count in the existing 32 voices / 32 clips / 32 MiB per mod (128 / 128 /
  128 MiB in all) together with the Bevy voices, and share the mixer's voice budget with the game (a refused open
  is logged and the voice dropped).
- Lifecycle: a mod stopping, failing or reloading stops its native voices and drops its banks; a map change does
  too (as for Bevy voices); the game's sound restarting (an audio content change) forgets the old runtime's voices
  (never released into the new one), registers the banks again, re-opens looping voices from the start and drops
  started one-shots.
- Parity: a native mod voice draws nothing from the evaluator's random generator; without native mod voices the
  per-frame system returns at once (no private MixMap is built), so the game sounds exactly as before.

Proofs (`game_audio::mod_voices::tests`, data-gated where they need the install):
`the_private_mixmap_matches_the_retail_emitter_outputs`, `a_native_mod_voice_plays_through_the_mixer_with_retail_pan_and_send`
(panned right at 5 m right, dry and send, silent beyond its reach, non-positional centred, no send with
`reverb = false`, everything gone when the mod stops), `native_one_shots_end_and_fades_stop`,
`a_restart_reopens_loops_in_the_new_runtime`, `a_removed_native_mod_voice_restores_retail` (the same retail scenario
with and without a mod voice: bit-identical before it, different while it plays, bit-identical again from the frame
after the mod stops, the same retail voices), `mod_banks_slots_and_cleanup`; `skate-mods` `native_play_options`
(serde boundary: valid, invalid, unknown fields; native by default, `native = false` refusing the native fields, the
default reach) and `vm::tests::audio_rule_commands_deserialize_and_validate` (the Lua wrapper passes `native` through:
absent = default, `false`); `modding::audio::tests::default_routing_needs_the_native_audio` (no native audio: the
Bevy voice) and `default_routing_is_native_until_the_native_voices_are_taken` (data-gated: native while the runtime
runs and a native voice is free, the Bevy voice once the 24 are taken, a key keeps its native voice). No-mod e2e
bench: byte-identical to run `m2` (84 outputs).

## I. Tuning writes at run time (capability `audio_tuning` = 1)

Status: built on `audio/moddability-2` (2026-10-04). Static tuning changes still go in `audio.json` `tuning` (B);
this is for mods that change tuning while they run.

```lua
sdk.audio.set_tuning('world', {traffic_engine = {c04_taxi01 = {idle_rpm = 1800}}})
sdk.audio.set_tuning('reverb', {A2782D75A971CC8C = {['5'] = 3.0}})     -- reverb01's value 5 (offset 20)
sdk.audio.set_tuning('player', {wheel_bucket_high = 4})
sdk.audio.set_tuning('world', nil)                                      -- restore this mod's world patch
sdk.audio.tuning('taxi', 'world', 'traffic_engine/c04_taxi01')          -- sdk.commands.result('taxi').value
local mine = sdk.audio.tuned()                                         -- fields this mod owns
```

- **Domains** (typed; `skate_mods::audio_tuning`): `player` = `player_tuning` (surface table, jitter, seams, grinds,
  wheel buckets, audio tricks, landing / collision materials and the Contacts posters' values), `world` =
  `world_tuning` (traffic engine records and model map, ped footsteps / objects / models, speech event tuning,
  speech voice curves), `bus` = `bus_tuning` (reverb presets, eEQChain bus records, FlangeSub returns), `reverb` =
  `bus_tuning.reverb` (preset key → its 44 values). The MixMap's layout (instances, pools, the 5 emitter states) is
  not a domain: it needs a restart and a deliberate non-retail option.
- **A patch** is a strict field merge onto the section as loaded (install + content overlays): tables by field,
  arrays by decimal index, leaves replaced by leaves of the same type (numbers, booleans, strings, number arrays of
  the same length). Checked when the command runs: a field or index the install lacks, another type, a field another
  mod owns, or a merged section that does not read back as the typed tuning is a command error (use
  `sdk.commands.request` to get it as a result). Bounds: 512 leaves, depth 8, 64 KiB, finite numbers within ±1e9.
- **Applied between audio passes**: at the start of the next pass (after a restart, before any post), the patched
  sections are rebuilt (mods in mod-id order; the first mod to write a field owns it), replaced in the `Library` and
  handed to the systems that cache them: the local player's components (the NPC skaters read the local player's
  tuning), the world host's ped tuning, the speech managers' event tuning and voice curves (their "who spoke when"
  timers kept), the world bridge's engine records and tazer hold, and the runtime's reverb presets / eEQChain records
  / FlangeSub returns (only the parts that changed: re-applying a FlangeSub preset restarts its LFOs). Systems that
  read the Library every frame see the new values at once.
- **Restored** when the mod stops, fails or reloads, or sets `nil`: the section goes back to the very value loaded
  (kept, not re-read) and the caches follow. Patches are kept across map changes; after an audio restart (a new
  Library) they are applied again. Not retuned in place: SFXObj_Jitter keeps the walk it was built with, and a
  reverb preset already faded in keeps its values until the network selects a preset again.
- Without patches the per-pass step returns at once: the game sounds exactly as before.

Proofs: `game_audio::tuning::tests::tuning_writes_apply_between_passes_reach_the_systems_and_restore_exactly`
(data-gated: errors at set, nothing before the pass boundary, the taxi record / the player's components / the
runtime's reverb preset take the values, reads, two mods own different fields, `nil` and the mod stopping restore
the loaded values exactly (world tuning field for field, player tuning, runtime presets), a restart applies the
patch again); `skate-mods` `audio_tuning` tests (shape, strict merge, claims) and
`audio_tuning_commands_deserialize_and_validate` (serde boundary, inspect path, Lua wrappers, capability).
No-mod e2e bench: byte-identical to run `m2` (84 outputs).

## J. Mod emitters and reverb zones (capability `world_audio` = 2)

Status: built on `audio/moddability-2` (2026-10-04). Two new kinds in the world-audio lifecycle (doc 15): keys,
limits (48 objects per mod, 128 in all), cleanup and `read` work as for cars and peds.

```lua
sdk.world_audio.spawn('fountain', 'emitter', {bank = 'water_fountain', patch = 81, position = {10, 0, 4},
                      extent = {6, 6, 6}, core = 0.2, volume = 0.5, falloff = 'squared'})
sdk.world_audio.spawn('cave', 'reverb_zone', {preset = 'BEEFC8E3DE04FBAE', position = {0, 0, 0},
                      extent = {30, 12, 15}, heading = 1.57})
sdk.world_audio.update('fountain', {volume = 0.3, position = {11, 0, 4}})
local f = sdk.world_audio.read('fountain')     -- {kind='emitter', audible=true (playing), ...}
sdk.world_audio.remove('cave')
```

- **Engine side:** components `crate::world_audio::WorldEmitter { bank, patch, extent, forward, core, volume,
  falloff }` and `ReverbZoneVolume { preset, extent, forward, core }` on any entity with a `GlobalTransform`; engine
  systems add them exactly as the mod command does. Read back: `WorldEmitterStats { playing, zones }`.
- **An emitter** is an `.ems` eVolumeType 1 record added after the map's records in the live list
  (`game_audio::emitters`): retail's reach test (a sphere of radius `extent[0]` when the three extents are equal,
  else an ellipsoid with semi-axes along `forward` (turned by the entity's rotation), up and side; the inner `core`
  at full level), level = `volume` × the falloff curve, the `c_emitter` post with the MixMap Emitter words (dry,
  send, pan, pitch, low-pass, the patch as selector), redelivered every frame, released when the listener leaves.
  The bank must be in the audio (a retail bank, or one a content overlay adds with `add.banks`); an unknown bank is
  a command error.
- **Emitter states (user decision 2026-10-04: "modding isn't in retails, so 'not like retail' isn't a thing. Yes
  default to seperate slots."):** retail has 5 emitter states (`CSTATEMGR_Emitter`, the MixMap's 5 Emitter
  instances). **Default `"extra"`: published emitters take their own instances of the private MixMap (H)**, so the
  map's emitters keep all 5 and every mod emitter in reach plays (up to the 32 instances shared with native mod
  voices; beyond that the next waits for a free one), with the same words (the private instances equal retail's).
  `settings/audio.json` `"mod_emitter_slots": "shared"` (or `SKATE_AUDIO_MOD_EMITTER_SLOTS=shared` for one run; the
  variable wins over the file, `extra` likewise) makes mod emitters share the 5 with the map's emitters instead, by
  retail's rule: nodes take states in the order they were reached, the map's records before published ones within a
  frame; when all 5 are taken the next waits. A settings file without the key gets the default.
- **A reverb zone** (eVolumeType 5) joins the zones the reverb selector walks (`ReverbZones`, `native::reverb_frame`
  → `skate_audio::bus::zones`), after the map's records, in the order reached; its id has bit 63 set and its own
  attribute key, so it never merges with a map zone. The preset must be one of the install's 24 `aud_reverb` keys:
  retail's zone walk stops at a zone whose attribute names no known preset, so a mod zone with an unknown preset is
  refused at spawn instead of silencing every later zone.
- Neither kind parks (they are records, not moving objects); `read(key).audible` = the emitter plays (holds a state
  or an instance) / the listener is inside the zone.
- Parity: a mod emitter's post runs the bank's program, which draws from the evaluator's one random generator (as
  every retail post), so with a mod emitter playing the retail random sequence differs from a run without it. With
  no published emitter or zone the per-frame tail rebuild does not run and the map's lists are exactly as before.

Proofs (data-gated, `game_audio::emitters::tests`): `published_emitters_share_retail_slots_play_and_release` (the
`"shared"` setting; 7 published at once: the first 5 in list order play, all 5 retail states taken; the bank's
program opens voices; a despawned one frees its state for the 6th; out of reach all stop; with none left no state
is held), `the_default_gives_published_emitters_their_own_instances` (the default settings: all 7 play, retail's 5
states stay free, the instances come back); `game_audio::tests::mod_emitter_slots_default_to_extra` (the default,
the file value, the variable overriding either way), `a_published_reverb_zone_selects_its_preset` (listed after the map's with its preset and a
published id, the retail selector fades to it, gone outside / despawned); `skate-mods`
`emitter_and_reverb_zone_options_validate` and the `world_audio_commands_deserialize` cases (serde boundary,
required fields at spawn). No-mod e2e bench: byte-identical to run `m2` (84 outputs).

## K. Mute / replace / layer rules (capability `audio_events` = 2, `audio_content` = 2)

Status: built on `audio/moddability-2` (2026-10-04). Lua cannot run inside the audio pass (the posts happen in the
middle of the game's audio frame), so changing a game sound is declarative: a mod states rules and the engine
applies them at the post sites, the same frame.

```lua
sdk.audio.rule('my_pop', {match = {tag = 'pop'}, action = 'replace', play = {path = 'audio/pop.wav', volume = 0.8}})
sdk.audio.rule('quiet_grind', {match = {tag = 'grind_start'}, action = 'mute'})
sdk.audio.rule('honk_layer', {match = {source = 'world', class = 'TRAFFIC_HORN'}, action = 'layer',
                              play = {path = 'audio/honk.wav', group = 'world'}, min_interval = 0.5})
sdk.audio.rule('my_pop', nil)                                        -- remove
```

or, without Lua, in `audio.json` (applied while the mod runs; rules alone never restart the game's sound):

```json
"rules": {
  "pop_click": { "match": { "tag": "pop" }, "action": "layer", "play": { "path": "audio/click.wav", "volume": 0.3 }, "min_interval": 0.2 }
}
```

**The rule format** (`skate_mods::audio_rules`, `deny_unknown_fields`):

| field | values | meaning |
|---|---|---|
| `match.tag` | `pop`, `land`, `grind_start`, `grind_end`, `footstep`, `horn`, `alarm`, `tazer`, `body_fall`, `emitter` | the event tags (E) on retail identities |
| `match.kind` | `post`, `splice`, `emitter_start` | a component / world post, a Splice sound start, a world emitter start |
| `match.source` | `player`, `world`, `npc`, `emitter` | the local player, the world host (traffic, peds), the NPC skaters, the world emitters |
| `match.class` | name | the retail class of a post, the bank of a Splice start or an emitter |
| `match.slot` | name | the poster's slot (`grind`, `wind`, `footstep`, `horn`, `ped_footstep`, `ring`, `body_fall`, …) |
| `match.id` | integer | the slot index (posts), the sound id (Splice), the patch (emitters) |
| `action` | `mute`, `replace`, `layer` | see below |
| `play` | `{path, volume 0..1, pitch 0.25..4, reverb (true), group ('player' / 'world'), at, offset, position, falloff}` | the mod's WAV (PCM16, 30 s, 8 MiB), required for `replace` / `layer`, refused for `mute`; where it plays: below |
| `min_interval` | 0..10 s (0.05) | the rule's sound plays at most once per interval |

At least one `match` field; every given field must hold. The first matching rule decides (mods in mod-id order;
per mod the `audio.json` rules, then the runtime ones, by key). 32 rules per mod, 64 in all.

**What each action does at each site:**
- A component / world / NPC **post** (`PlayerAudio::apply`, `world_sources::apply`, `NpcHost::apply`): `mute` skips
  the post, so no node is held and the component's later redeliveries and its release find none and do nothing;
  the component itself runs unchanged.
- A **Splice start** (pops, landings, foot plants, foley, collisions, ped rings / falls; the `Observed` Splice
  wrapper): `mute` returns "not started" to the component (the same as a missing bank).
- A world **emitter start** (`emitters::update`): `mute` keeps the node and its emitter state (the slot use stays
  retail's) and posts nothing.
- `replace` = `mute` + the rule's sound; `layer` = the game's sound + the rule's sound. The rule's sound is a native
  one-shot (H) of the rule owner's bank, opened in the same pass for requests made before `mod_voices::frame` (the
  local player's process and update, the world / NPC owners' process) and in the next pass for later ones (their
  update, the emitters); a positional one is heard from the pass after its first position (the private MixMap
  evaluates a position before its words are read, as for every native mod voice): at most two game frames after
  the request. Each rule has a ring of 4 voices.

**Where the rule's sound plays** (user decision 2026-10-04: "Yes it should be positional, and mods should allow them
to choose where positionally that audio should appear."; until then it played centred). `play.at`:

| `at` | position | fields |
|---|---|---|
| `owner` (default) | the owner of the game's sound, followed every frame while the sound plays: the local skater's centre of mass (the position retail's player sounds follow, MixMap 3DObjPos 60010010), the car or ped (`WorldOwners`), the NPC skater's centre of mass (`NpcSkaters`), the emitter record's position (an emitter start) | `offset` [x, y, z] m added to the owner's position (world axes, y up; −100..100) |
| `world` | a fixed world position | `position` [x, y, z] (required; ±100 km) |
| `centre` (alias `center`) | non-positional, centred: the retail non-positional emitter outputs (out2 dry, out7 send, out1 pitch, out3 low-pass), the behaviour before the decision | none |

A positional rule sound is a native mod voice with a position (H): the private MixMap's Emitter instance gets the
position the way a retail emitter state does (`native::write_position`: listener, skater, source), so it has the
retail emitter law: the dry level (out4, with the global ducks), the camera-azimuth pan (out0), pitch and low-pass,
and the environment send rolling off with camera distance 4 → 70 m. Its level is `volume` × the reach curve
(`falloff` = `{radius, core, curve}` as in H; refused with `centre`). Without `falloff` the reach is the owner's
retail one: an emitter start takes the emitter record's own shape (sphere or ellipsoid, core) and falloff curve,
exactly the reach the replaced emitter has; a car 40 m (retail's traffic list cut, `TRAFFIC_LIST_RADIUS`); a ped 50 m
(the ped list cut, `PED_LIST_RADIUS`); a skater 30 m (retail's skater audio radius, `skaters::AUDIO_RADIUS`; the local
player too, whose sounds sit in the same Player MixMap slot); all with the squared curve. With `at = 'world'` the reach
still defaults to the owner's. An owner that is gone while its sound plays (a car despawned) leaves the sound at its
last position; an owner never found (gone before the sound opened) plays it centred. Emitter records do not move: the
sound stays at the record's position. Checked when the rule is set / the overlay is read, before the WAV is loaded:
`offset` only with `owner`, `position` exactly with `world`, `falloff` not with `centre`, the ranges.

```lua
sdk.audio.rule('honk', {match = {tag = 'horn'}, action = 'replace',
                        play = {path = 'audio/honk.wav', group = 'world', offset = {0, 1.2, 0}}})      -- at the car
sdk.audio.rule('beacon', {match = {tag = 'land'}, action = 'layer',
                          play = {path = 'audio/ping.wav', at = 'world', position = {10, 0, 4}, falloff = {radius = 30}}})
sdk.audio.rule('ui_pop', {match = {tag = 'pop'}, action = 'replace', play = {path = 'audio/pop.wav', at = 'centre'}})
```
- Event rows report the game's requests: a muted request is still delivered to subscribers (a "custom pop" mod
  mutes `pop` and still sees every pop).
- Parity: a muted post or Splice start does not run its program / sound, so the evaluator's shared random sequence
  differs from a run without the rule (mod-only, as for mod posts). `layer` keeps every retail request; its sound
  draws nothing. With no rules every site holds `None` and checks one branch: the game sounds exactly as before.

Proofs: `game_audio::mod_rules::tests` (matching, first rule decides, mute / replace / layer verdicts, the
min_interval, the voice ring; runtime limits and cleanup; `audio.json` rules compile with their WAV in the mod's
bank, a rules-only overlay never restarts the sound, a rule with a missing WAV is left out; the mod-side tags are
engine tags); data-gated: `player_audio::tests::rules_mute_and_layer_the_players_pop_grind_and_landing` (fewer
contact voices in the pop frames with `pop` muted, none with the bank muted, no grind node with `grind_start`
muted, the rows unchanged, the landing's layered sound queued once, and without rules the output is bit-identical to
a run before rules existed), `world_sources::tests::rules_mute_replace_and_layer_the_world_hosts_posts` (no horn node
while muted, rows and the ped's steps unchanged, replace = dropped + one sound, layer = kept + one sound),
`emitters::tests::a_rule_mutes_an_emitter_start`, `mod_voices::tests::a_rule_sound_plays_in_the_mod_bank`;
positional (2026-10-04): `mod_rules::tests::rule_sounds_are_placed_at_their_owner` (the request's owner travels
with the sound: the local skater, a car by id with an offset and the mod's reach, an NPC skater's request at a fixed
position, centred, an emitter record with its own reach; an emitter request without its record centred),
`owners_are_located_with_their_retail_reach` (skater 30 m, car 40 m, ped 50 m, fixed record; unknown ids not found),
data-gated `mod_voices::tests::a_positional_rule_sound_plays_at_its_owner` (a replaced horn panned right at its car
5 m right with the send, following the car left, staying where the car was when it is gone, silent beyond the car's
40 m; an offset, a fixed world position and `centre` heard where they say); `skate-mods`
`rule_placement_options_validate` (each `at` / `offset` / `position` / `falloff` combination, ranges, typos),
`rules_parse_and_validate`, `rules_in_audio_json_are_checked_and_carry_no_content` (check_mod's deep
validation: the WAV is read and counted), `audio_rule_commands_deserialize_and_validate`. No-mod e2e bench:
byte-identical to run `m2` (84 outputs).

## L. The follow-up (H–K): implementation, verification, open questions

Problem: after the first modding cut (then PR #36, now part of #32) a mod could still not route its own sounds through the game's mixer, change tuning
while it runs, add emitters or reverb zones, or change the game's own sounds. Root cause: mod WAVs had only the Bevy
path; tuning was read once at start; the emitter and zone lists were the map's only; the post sites had no hook a
mod could reach (Lua cannot run in the audio pass).

Changes (branch `audio/moddability-2`, then PR #43, on top of the PR #36 branch; both now part of #32):
- `crates/skate-game/src/game_audio/mod_voices.rs` (new): per-mod mixer banks, native mod voices driven by the
  retail emitter words, the private MixMap (`ModMix`); `emitters.rs` `sphere_level`; `native.rs` `write_position`
  public, a test helper.
- `game_audio/tuning.rs` (new) + `library.rs` (`tuning_base`, `check_tuning`, `set_tuning`, `restore_tuning`; the
  loaded sections kept for an exact restore) + `retune` on `PlayerAudio`, `WorldHost`, `WorldSpeech`, `Bridge`,
  `AudioApi`; applied from `content::frame` (between passes).
- `game_audio/emitters.rs`: the published `WorldEmitter` / `ReverbZoneVolume` tails, the shared / extra emitter
  states, `WorldEmitterStats`; `world_audio.rs` components; `game_audio/mod.rs` `mod_emitter_slots` setting.
- `game_audio/mod_rules.rs` (new): `AudioRules`, `RuleSet`; the checks at `PlayerAudio::apply`,
  `world_sources::apply`, `NpcHost::apply`, `mod_audio::Observed::start`, `emitters::update`; `content.rs`
  (`rules_generation`; only content restarts the sound).
- `crates/skate-mods`: `audio.rs` (native play options), `audio_tuning.rs` (new), `audio_rules.rs` (new),
  `audio_content.rs` (`rules`, `has_content`), `world_audio.rs` (the two kinds), `vm.rs` (commands `audio_set_tuning`,
  `audio_rule`, inspect `audio_tuning:…`, capabilities `audio` 3, `audio_tuning` 1, `audio_events` 2,
  `audio_content` 2, `world_audio` 2), `api.lua` (`sdk.audio.set_tuning / tuning / tuned / rule`, native fields).
- `crates/skate-game/src/modding/`: `audio.rs` (native voices beside the Bevy ones, shared limits, positions),
  `world_audio.rs` (the two kinds, bank / preset checks, read-back), `mod.rs` / `engine_access.rs` (dispatch).
- `sdk/skate.lua`, `sdk/GENERAL_API.md`; `mods/audio-content-test` (dev, off by default): F8 native siren + beep,
  F9 tuning writes, F10 emitter + reverb zone, F11 a mute rule, `audio.json` `rules.pop_click`.

Verification: the headless e2e bench (13 scripted scenarios and 4 whole sessions, the 60 Hz host and 300 fps; 84
renders and voice logs) is byte-identical to the PR #36 head's run after every step (S1–S4); the tests named in H–K;
all suites (skate-audio with ignored, game_audio with ignored and the private-data env, skate-mods, the build).
The three defaults (2026-10-04): the bench on the branch head before them (run `h0`, after the merge of the PR #36
branch) and after them (run `p1`) is byte-identical, 84 of 84 (26 scenario renders and voice logs at the 60 Hz host
and at 300 fps each, 16 whole-session ones each), and both equal run `m2`: with no mod the sound does not change.
Suites after them: skate-mods (all but the known `skyline_every_component_is_real_and_drives_through_ground_contact`,
whose GLB is not in the checkout), skate-audio with ignored, the `game_audio` and `modding` tests with ignored and the
private-data env (61 data-gated, all passing; `SKATE3_ASSET_ROOT` for the rig test), `cargo build --locked`;
`check_mod --install` for `mods/audio-content-test` (12 identities, 2 rules) and `sdk/examples/audio-example`:
0 warnings, 0 conflicts. Not checked in game yet: the Skyline car's and the dev mod's sounds on the native path
(levels differ from the Bevy voices), the positional rule sounds by ear.

Decisions (the user, 2026-10-04; the six questions the first cut was built with, each answered):
1. **Mod emitters get their own emitter instances by default** (J): `"mod_emitter_slots"` defaults to `"extra"`;
   `"shared"` (retail's 5 with the map's emitters, first reached first served) stays selectable. The user: "modding
   isn't in retails, so 'not like retail' isn't a thing. Yes default to seperate slots."
2. **Mod WAVs go through the native mixer by default** (H; spec L6): no `native` field = native (a Bevy voice when the
   native audio is missing or its 24 voices are taken), `native = false` = the Bevy voice, `native = true` = native
   only. The user: "Yes duh".
3. **Rule sounds are positional, and the mod chooses where** (K): by default at the owner of the game's sound,
   following it, with the owner's retail reach; `play.at` = `owner` (+ `offset`) / `world` (+ `position`) /
   `centre`, `play.falloff` for the reach. The user: "Yes it should be positional, and mods should allow them to
   choose where positionally that audio should appear."
4. **Tuning patches are kept across map changes** (I), as built (globals are still restored at a map change).
5. **A muted request is still an event row** (K), as built: a "custom pop" mod mutes `pop` and still sees the pops.
6. **Empty / rules-only overlays don't restart the sound** (K), as built.

Implementation of 1–3 (2026-10-04, on `audio/moddability-2`):
- 1: `game_audio/mod.rs` (`ModEmitterSlots` default `Extra`, `mod_emitter_slots()` with the
  `SKATE_AUDIO_MOD_EMITTER_SLOTS` override either way), `emitters.rs` (reads it), `world_audio.rs` docs.
- 2: `skate-mods` `audio.rs` (`native: Option<bool>`, `DEFAULT_REACH`, `wants_native`), `api.lua` (passes `native`
  through), `modding/audio.rs` (`play`: native unless `false`, the Bevy fallback for the default, the default reach; `game_audio/native.rs` `start_for_test`, a test helper), `vm.rs` (a Lua case).
- 3: `skate-mods` `audio_rules.rs` (`RulePlay` `at` / `offset` / `position` / `falloff`, `RuleAt`, checks);
  `game_audio/mod_rules.rs` (the request's owner travels with the queued sound: `Origin`, `mutes_at` for the emitter
  site's position and reach; `take_plays` builds the placement); `mod_voices.rs` (`Reach` replaces the sphere tuple,
  `Follow` / `Anchor`, owners located every frame from `Cues`, `WorldOwners`, `NpcSkaters`; the owner kinds' retail
  reach); `emitters.rs` (`shape_level`, the emitter start passes its record's position and reach);
  `modding/mod.rs` / `game_audio/mod.rs` `set_rule` (checks before the WAV loads).
- `sdk/skate.lua`, `sdk/GENERAL_API.md`; `mods/audio-content-test`: F8 without `native` / `falloff` (the defaults),
  F10's emitter on its own instance (the default), F11 adds a landing replaced by a beep at a fixed world spot,
  `audio.json` `pop_click` at the owner and `honk_beep` 1.5 m above each honking car.

## M. The third pass: hot swap, MixMap inputs and instances, Csis projects, seed, hot reload (L1–L5, L7)

Status: built on `audio/moddability-2` (2026-10-04, on top of H–K; uncommitted at the time of writing). With these
the "Later" list of section 3 is done (L6, native routing by default, was done in H). Capabilities: `audio` = 4
(L2, L5), `audio_content` = 3 (L1, L4, L7), `audio_events` = 3 (the offset frame, moving emitters), `world_audio` = 3
(L3), then 4 (own instances the default for mod cars and peds, M3). Without mods nothing here runs: the headless e2e
bench is byte-identical (below).

### M1. Hot swap instead of the restart (L1)

Problem: a mod's audio content coming or going restarted the whole native runtime: every sound was cut and every
held post, emitter, voice and speech line started again. Root cause: the content overlay rebuilt the `Library` and
the only way to bring it into the runtime was `content::restart`.

Change: `content::swap_or_restart` (`game_audio/swap.rs`) compares the running library with the new one and
replaces only what changed, in place, in the same runtime:

| what changed | in place |
|---|---|
| an AEMS bank the runtime holds (its `.abk`, WAVs by slot, rebuilt headers, volume group) | `Runtime::replace_bank` / `Evaluator::replace_bank`: the bank keeps its runtime id and its place in each class's constructor list; its instances go (voices released); every post its poster still holds is re-bound to the new bank with the post's last payload (the longest ClassData copy of the post), so a held traffic engine, emitter or player layer continues on the new content. No random draws. A bank that left the audio is unloaded; banks not loaded load from the new library on use. |
| overlay-preloaded banks | the new set loads now |
| a Splice tree (pops, landings, foley, the ped ring) or its WAVs | `SplicePlayer::replace_bank`: same index and mixer bank; its sounding sounds stop (as after a release) |
| the wheel streams | `Runtime::load_streams` again |
| mod Csis projects (M4) | installed / taken out of the lookups (`Registry::uninstall`); the banks bound to them re-bound |
| tuning sections from overlays | handed to the systems that cache them (`tuning::retune`, as a tuning write) |
| speech index / takes | `WorldSpeech::reload_content`: the lines speaking stop, the data is read again |
| the world layer (beds, zones, crossfades, regions, `.ems` records, sets, map audio: `Library::world_key`) | `AudioContent::world_generation`: the map-keyed state (emitters, reverb zones, zone ambience) rebuilds; the emitters release their nodes in the same runtime and do **not** unload the map's banks (no epoch: the world / NPC hosts keep playing) |

"Exact" here means: after a swap the runtime holds, for everything it has loaded, exactly what a restart would load
(the same `.abk` bytes, sample headers, PCM, groups, class bindings in the same constructor order, Splice trees,
streams, tuning, projects); only the sounds of the replaced content restart. Kept as the **restart fallback** (the
plan's reasons are logged and shown in `sdk.audio.info().last_change`):
- the **MixMap file** (its controller graph and every instance's state are built from it);
- the **rolling bed's** grain recordings or grain tuning (the bed is built at the runtime's start);
- the **install's own projects** (overlays never change them; a guard), a Splice tree that left the audio (a guard),
  and any error while swapping.

Identities that live as long as the runtime now key on `AudioContent::runtime_generation` (bumped by restarts only):
mod post handles (`mod_audio`), native mod voices and the private MixMap (`mod_voices`), the emitters' node
forgetting. `generation` is bumped by every content change (the new library: tuning re-applied, layouts and sets
rebuilt). Mod files are stamped (size and modification time) when the library loads (`Library::stamp`), so a file
edited in place is new content.

Files: `crates/skate-audio/src/eval/mod.rs` (`bind`, `replace_bank`, `payload_of`, `banks_using_project`,
`uninstall_project`), `eval/symbols.rs` (install tokens, `records_of`, `uninstall`), `runtime.rs` (`replace_bank`,
`install_project` returns the token), `mixer.rs` (`bank_group`), `splice/mod.rs` (`replace_bank`);
`crates/skate-game/src/game_audio/swap.rs` (new), `content.rs` (`swap_or_restart`, `merged`, `restart_with`, the
new counters, `last_change`), `library.rs` (`stamp`, the content keys `bank_key / splice_key / wheels_key /
mixmap_key / grain_key / speech_key / world_key / project_files / tuning_sections`), `native.rs` (`replace_bank`,
`unload_bank`, `set_resident`, `bank_ids`, `mod_projects`), `world_speech.rs` (`reload_content`), `emitters.rs`,
`ambience.rs`, `mod_voices.rs`, `mod_audio.rs` (`swaps`, `last_change` in the info), `tuning.rs` (`retune` shared).

Verification: `eval::tests::a_replaced_bank_keeps_its_place_and_re_instances_held_posts` (the constructor place,
only the held post re-instanced with its payload and playing its slot at the next walk, a released post not, the
generator untouched; with the project out-take below); data-gated
`content::tests::a_mod_is_swapped_in_and_out_without_a_restart` (the same runtime and stream, the same bank id and
constructor list, the runtime holds exactly the new library's bank, a held emitter post re-bound and sounding on the
mod's samples while a second world without the mod differs, an unrelated held post keeps its instance; the WAV
edited in place is swapped again with its new header; the mod out = the install's bank again; a MixMap replacement
restarts with the reason); the restart test (`a_restart_mid_scenario_…`) calls the restart explicitly and still
proves restart = fresh start.

### M2. Writable MixMap inputs (L2)

Problem: a mod could read MixMap outputs but not drive the controllers, so a "duck the world" mod could only scale
a group volume, outside retail's curves. Change: `game_audio/mixmap_inputs.rs` (`MixMapInputs`):
`sdk.audio.set_mixmap_input(slot, object, instance, input, value | nil, {float})`. Checked at the command (slot name,
object 0..127, instance 0..31, input 0..15, an integer word or a finite f32, the controller must exist in the game's
MixMap); owned (first mod per input), 16 per mod, 64 in all. Applied in `native::mixmap_tick` after every host write
of the pass and before the evaluations, so a value holds against inputs the host writes (the Master gains) as well
as the duck flags it never writes. Released by `nil` / the mod stopping, failing or reloading: the input gets the
value it had before the first write; kept across map changes; written again over a restarted runtime's MixMap.
Read back: `sdk.audio.mixmap_inputs()` (`snapshot.audio[mod].inputs`), `info().mixmap_inputs`.
Verification: data-gated `mixmap_inputs::tests::mixmap_inputs_drive_retail_controllers_and_release_exactly` (the
checks, an emitter instance at 5 m ducked through the Master gains, held against the host's write, first owner
wins, the release gives the retail level back exactly, the limits); `skate-mods`
`vm::tests::mixmap_input_and_seed_commands_deserialize_and_validate`.

### M3. Extra MixMap instances for world objects (L3)

Problem: published cars and peds compete with the map's for retail's 4 traffic / 15 pedestrian instances (8 / 24
more-audible): a mod's sixth car near the camera is silent. (Mod emitters and voices already had their own private
instances, H / J.) Change: `game_audio/mod_world.rs`: an object with `world_audio::OwnAudioInstance` is published by
the bridge to `OwnWorldOwners` instead of `WorldOwners` and played by a second `WorldHost` (the same retail traffic /
ped objects, posts, Splice steps, speech requests, rules and event rows) on a private MixMap (the install's file,
Global + 16 Traffic + 16 Pedestrian instances, Global inputs copied from the game's before each evaluation). The
retail pools never see them. `world_sources::pre_in / post_in` take the MixMap and pool sizes (the game's host passes
its own: unchanged). NPC skaters stay in retail's Player slot (open, M9 Q1).

**The default for mods (user decision 2026-10-04, M9 Q2: "yes I agree it should match", after "modding isn't in
retails, so 'not like retail' isn't a thing. Yes default to seperate slots."; capability `world_audio` = 4):** a
mod's car or ped takes its own instance unless it asks otherwise, as its emitters do (J):

| `slots` (spawn only; cars and peds) | instances | |
|---|---|---|
| none / `'own'` | its own instance of the private MixMap | the default since `world_audio` 4; `'own'` stays valid (the opt-in of 3) |
| `'shared'` (alias `'retail'`) | retail's pools, shared with the map's objects: the nearest win | the default of `world_audio` 3; `'retail'` works on 3 and 4 alike |

Any other value, or `slots` on a skater, emitter or reverb zone or in an update, is a command error.

- **Only mod objects change.** The mod command (`modding/world_audio.rs` `spawn`) adds `OwnAudioInstance` to a mod's
  car or ped by default (`skate_mods::world_audio::Slots::resolve`). The engine-facing path is unchanged: an engine
  system publishing the living world adds `TrafficAudio` / `PedAudio` without the marker and stays in retail's
  pools; an engine object that must be heard adds the marker itself.
- **A full private pool** (more than 16 own cars or 16 own peds of all mods inside the list radius, 40 m horizontal /
  50 m 3-D): the 16 nearest hold the instances (retail's own rule, `owners::Pool`), the others **wait**, silent, and
  take an instance as soon as they are among the 16 nearest (a nearer one moves away, goes or the listener moves).
  Chosen over the alternatives because a mod may publish 48 objects and only the ones in reach compete: refusing the
  17th spawn would break a mod that publishes a street (the world-audio test mod spawns 20 peds), and spilling into
  retail's pools would take the map's instances, which own instances exist to keep. It matches the emitters (J: past
  the private instances the next waits). Readable: `read(key).waiting` (in reach, no instance: every one is held by
  a nearer object; the same for `shared` objects in a full retail pool), `info().own = {instances = {traffic = 16,
  peds = 16}, published, audible, waiting}`, `info().waiting` (retail's pools), `read(key).slots` (`'own'` /
  `'shared'`, the object's setting; `own` stays "holds an own instance"); the log warns once when waiting starts
  (`AUDIO_WORLD own instances full: …`) and the per-second `WORLD_AUDIO own instances: …` line while own objects exist.
  Engine side: `WorldAudioStats { waiting, own_vehicles, own_peds, own_traffic_held, own_peds_held, own_instances,
  own_waiting }` (the hosts' `WorldHost::waiting`, `WorldHeld::{waiting_*, own_waiting_*}`: bookkeeping only).
- Read back as before: `WorldAudioInstance.own`, `read(key).own`.

Verification: data-gated `mod_world::tests::own_instance_cars_all_play_beside_retails_pool` (six own cars all hold
an instance and post, the game's pool holds none, released when they go),
`mod_world::tests::a_full_private_pool_lets_the_nearest_play_and_the_rest_wait` (18 own cars in reach and one beyond
40 m: the 16 nearest hold, the 2 farther wait, the far one does not, retail's pool holds none; one near car goes and
the nearest waiting car takes its instance at the next pass; 16 in reach: nobody waits);
`world_sources::tests::a_full_pool_leaves_the_farther_ones_in_reach_waiting`;
`modding::world_audio::tests::mod_cars_and_peds_default_to_their_own_instance` (default own for cars and peds,
`shared` / `retail` none, explicit `own`, skaters never, a respawn with `shared` drops it, an engine-published car has
none, `read(key).slots`); `world_bridge::tests::own_instance_objects_go_to_their_own_host` (the marker routes, an
unmarked car goes to retail's pool); `skate-mods` `world_audio::tests::slots_default_to_own_and_shared_opts_in`, the
`world_audio_commands_deserialize` cases (`shared`, `own`, `retail` at spawn; `slots` in an update refused), the
capability test (`world_audio` = 4). No-mod e2e bench: byte-identical to run `m2` (84 outputs).

### M4. Mod Csis projects (L4)

Problem: a mod bank could only bind to a retail class. Change: `audio.json` `add.projects` (`.csi` files, at most 8):
checked by the Csis parser (`check_file`, a project without symbols refused), appended after the install's
projects (`audio_merge`); when the overlays merge (game and `check_mod --install`) a project whose symbol name (per
table: function, class, global) is the install's or an earlier mod's is a clash and the whole overlay is left out
(`audio_content::project_clash`; a mod name can never shadow a name the game or another mod posts by). Installed at
the runtime's start after the install's (`Native::mod_projects` keeps each one's registry token), and in a hot
swap: a new project installed, one that went taken out of the lookups (`Registry::uninstall`: every reference and
game-side name resolves as if it had never been installed; its records stay, emptied, ids are indices), the banks
bound to it re-bound or unloaded. `skate_audio::formats::csi::Project::to_bytes` writes projects (tools, tests; the
dev mod's `synthesize.py` has the same writer).
Verification: data-gated `content::tests::a_mod_csis_project_is_installed_and_taken_out_without_a_restart` (class and
global resolve, the global's default, a post to the class makes the mod bank's instance, retail lookups unchanged,
out again by a swap, a clash with `c_emitter` rejected, a restart installs it after the install's);
`skate-mods` `audio_content::tests::mod_csis_projects_are_checked_and_merged`, `csi::tests::a_written_project_reads_back`.

### M5. A seedable audio random state (L5)

Problem: the audio's draws are deterministic from the start but a mod test cannot start them from a known point.
Change: `game_audio/seed.rs`: `sdk.audio.seed(n | nil)` (or `SKATE_AUDIO_SEED=<n>` for a run) sets **every**
generator from one number (splitmix64 per generator): the evaluator's, the Splice player's, the grain bed's, the
eEQChain buses', the Jitter walk's, the world host's and the speech host's; at the start of the next pass. One owner;
`nil` puts back the states from the first seed; a restarted runtime is seeded again. Unseeded nothing runs.
Verification: data-gated `seed::tests::a_seed_makes_runs_reproducible_and_unseeded_is_retail` (unseeded: no state
changes; equal worlds whose generators were scrambled before the seed point render bit-identically after the same
seed, another seed differs on an emitter program that draws; release restores; a restart is re-seeded); the no-mod
bench.

### M6. Hot reload of `audio.json` (L7)

Problem: editing a running mod's `audio.json` (or a WAV it names) reloaded the whole mod (its script restarted) and
then restarted the game's sound. Change: the mod manager (`skate_mods::Manager::scan`) now knows which files changed
(`Fingerprints::get_with_changes`); when every changed file is `audio.json` or a file the old or new `audio.json`
names (and `mod.json` is unchanged), the package's fingerprint is updated **without** stopping the script and the
changed files are handed to the game once (`Package::take_audio_changes`). The game drops the native clips of those
files (`ModVoices::forget_clips`: re-read at the next play or rule compile) and the content overlay is read again and
hot-swapped (M1). A changed file the script holds as a Bevy clip reloads the mod as before; any other change too.
Verification: `skate-mods` `general_api_tests::audio_only_edits_keep_the_script_running` (the script's `on_load`
ran once across an `audio.json` edit and edits of WAVs the old and the new `audio.json` name; another file reloads
it); the in-place WAV edit in the swap test (M1).

### M7. Offsets in the owner's axes; moving emitters

- `play.frame = 'owner'` (rules, `at = 'owner'`): the `offset` is x = the owner's right, y = up, z = its facing,
  turning with it (`mod_voices::owner_offset`; facing: the skater's board from its wheels (front pair minus back
  pair, else its velocity), a car's direction, a ped's velocity, an emitter's forward; flattened to the ground,
  +Z without one). Default `world` (unchanged). Validation: only with `at = 'owner'`.
- A rule sound at a **published** emitter (`WorldEmitter`) follows the entity when it moves
  (`Anchor::Emitter`, located every frame from its transform, its reach turned with it); the map's records stay
  fixed. The published emitter's own sound already followed (the published tail is rebuilt from the transforms every
  frame): now proven.
Verification: `mod_voices::tests::offsets_turn_with_the_owner`, `mod_rules::tests::rule_sounds_are_placed_at_their_owner`
(the published emitter's entity travels with the sound), data-gated `emitters::tests::a_moving_published_emitter_is_followed`
(the same post, its instance's camera distance 3 → 8 m as the entity moves, released out of reach); `skate-mods`
`rule_placement_options_validate`.

### M8. Verification (the whole pass)

- No-mod identity: the headless e2e bench (13 scenarios + whole sessions, the 60 Hz host and 300 fps; 84 renders
  and voice logs) on the branch head before the pass (run `q0`, = `m2`), after L1 + L7 (`r1`), after L2 / L5 / M7
  (`r2`) and after L3 / L4 (`r3`): every run 84 of 84 byte-identical to `q0` and `m2`. (Bench from a saved test
  binary of the worktree's own target, `run_exe.sh`.)
- Suites: skate-audio with ignored (all pass, `render_alloc` included: the render path stays allocation-free); the
  game's `game_audio::` tests with ignored and the private-data env (133 pass) and `modding::` (32 pass); skate-mods
  (all but the known `skyline_every_component_is_real_and_drives_through_ground_contact`, whose GLB is not in the
  checkout); `cargo build --locked` with no new warnings (the 4 known).
- `check_mod --install`: `mods/audio-content-test` (12 identities, its project 3 symbols, 0 warnings, 0 conflicts),
  `sdk/examples/audio-example` (0 warnings); Skyline and the world-audio test mod pass (no `audio.json`).
- Dev mod `mods/audio-content-test`: `audio.json` adds its project `audio/dev.csi` (made by `synthesize.py`;
  `.gitignore` excepts it), `honk_beep` 1.5 m above and 2 m ahead of each honking car (`frame = 'owner'`); keys
  Digit1 duck (Master inputs), Digit2 seed, Digit3 six own-instance taxis, Digit4 the mod's class + global, Digit5 a
  beep 2 m ahead of the board's nose on pops, Digit6 an orbiting emitter; a second HUD line shows swaps / restarts /
  the last change. Not checked in game yet.
- **The own-instance default (M3, `world_audio` = 4, 2026-10-04):** no-mod bench run `s5` 84 of 84 byte-identical
  to `q0` and `m2`; skate-audio with ignored all pass; `game_audio::` with ignored 135 pass, `modding::` 33 pass;
  skate-mods 96 pass (the known Skyline failure only); `cargo build --locked` the 4 known warnings; `check_mod
  --install`: `audio-content-test` and `audio-example` 0 warnings, 0 conflicts; the world-audio test mod and Skyline
  OK. Dev mods: `audio-content-test` Digit3 spawns its taxis without `slots` on 4 (`'own'` on 3); `world-audio-test`
  (which shows retail's limits) asks for `slots = 'shared'` on 4, and its new setting "Own instances" (`own_slots`)
  shows the default instead. Not checked in game yet.

### M8b. Automated in-game check (the dev mod's `autotest` setting, 2026-10-04)

Problem: checking H–M in game needed a person in DownTown pressing F5–F11 and Digit1–6. Change (dev mod only,
`mods/audio-content-test`, no engine code): mod settings `autotest` (off by default), `autotest_step` (s per step),
`autotest_gap` (quiet s before each step), `autotest_quiet_step` (s per silent read-back step, default 3),
`autotest_muted` (shows "MUTED TEST" on its HUD). With `autotest` on, once
the map's native audio runs the script presses its own keys on a timer (the same code paths as the keys), one step
after the other, reads back what the engine reports and logs one line per check: `AUTOTEST <check> ok|fail <details>`
(then `AUTOTEST done pass=N fail=M`). 16 steps: the map's Baby_Cry_1 emitter with the replaced bank (the camera, i.e.
the listener, is put 5 m from DownTown's record), F6 / F7 post and release (patch 22 at that emitter), F5 global,
F8 native siren + beep, F9 tuning writes (a mod taxi idles 3 m away: the game spawns no traffic), F10 emitter +
reverb zone, F11 rules with a synthetic ollie (gameplay actions: a push, then the right stick down / up), Digit1 duck,
Digit2 seed, Digit3 six own-instance taxis driving a 6 m circle, Digit4 the mod's Csis class and global, Digit5 the
nose beep with an ollie, Digit6 the orbiting emitter, event tags after subscribing again, and the L7 hot reload (it
asks the runner to edit `audio.json`: `AUTOTEST_REQ edit_audio_json` / `revert_audio_json`, and checks two swaps, no
restart and the script loaded once). Read-backs: `sdk.audio.info()` (native voices, rules, seed, MixMap inputs, swaps,
restarts, last change), `handle`, `global` (the watch is re-sent once the native audio runs: at `on_load` it does not
yet and the watch fails), `tuned` / `tuning`, `mixmap_inputs`, `sdk.world_audio.read / info`, `sdk.commands.result`,
the audio catalog (loaded banks) and the event rows. Every step has a hard limit (its length + 5 s: a
`<step>_timeout` fail line). Output levels of mixer voices are not readable from Lua: a check proves "held, posted,
started", the ear (or a headless render) proves the level.

A local runner (not part of the PR) starts the game on DownTown with a copy of the mod in a run folder and a private
`SKATE3_MOD_SETTINGS` folder (`<id>.json` = `{"enabled": true, "values": {"autotest": true, ...}}`), muted and
minimised by default or audible (15 s steps, 3 s gaps), edits / reverts the copy's `audio.json` when asked, stops the
game after `AUTOTEST done` and tabulates the lines; a second pass with the mod disabled asserts retail (`Map audio
DownTown: ["retail"]`, the retail crossfade bank, no overlay, the install's 9 Csis projects, no mod log line).

Easier to follow (2026-10-04, after the user's audible run: "the autotest is stuck" ... "it got stuck after teleporting
the camera"; the mod kept running, but the parked camera showed a still picture for steps 1-3, step 2 (F5) made no sound
and nothing on screen showed progress; and "it keot playing the beep even though it said it stopped it", then "oh its
because the camera was still there": the map emitter kept beeping while the camera stayed near it). Changes, still dev
mod + runner only:
- Each step has an `audible` flag. Steps with something to hear run `autotest_step` s (15 s audible); the silent
  read-back checks (F5 global, Digit2 seed, Digit4 Csis, the hot swap) run `autotest_quiet_step` s (3 s, at most the
  step length) after a gap of at most 1 s.
- The HUD counts down: `3/16 <step>  9 s left` (`finishing...` while a step waits for its last read-back), during gaps
  `Next in 3 s: 4/16 <step>`. Each step / gap start is logged as `AUTOTEST_HUD <text>`.
- The camera: steps 1-3 (the map emitter, F6 post, F7 release) carry a `camera` flag; F5 moved after them. While
  parked, the camera slowly circles the emitter at 5.1 m (one lap per 40 s, still within the 18 m reach) so the picture
  moves, and a cyan HUD line says "Camera moved to the Baby_Cry_1 emitter (on a building), circling it; the skater is out
  of view. Back at the end of step 3." It returns 1.5 s before the end of F7 (the map emitter's beep stops: out of reach),
  then the HUD says "Camera back at the skater". New check `camera_back`: an `emitter_stop` row of the map's Baby_Cry_1
  emitter after the return (the listener left its reach; in the muted run the engine's `AUDIO_EMITTER stop Baby_Cry_1 #0`
  came 4 ms after the return). The snapshot's camera position is logged too but not trusted (it may not report a
  mod-set camera, see gotchas). The camera is also returned when a step times out, when the next step does not need it, at
  the end, and in `on_unload` (the engine clears a mod's camera when the mod stops anyway). The camera override does
  not freeze the game: the skater keeps its input and physics, only the view is fixed (hence the circling).
- The step texts say which beep is which: the map's Baby_Cry_1 emitter (replaced samples, plays while the camera is
  near it) vs the mod's centred F6 post.
- The runner adds `hud_countdown` (>= 16 `AUTOTEST_HUD` lines with a countdown) and `camera_returned` (the log line).
- Not checkable from Lua: whether the F7 post's voices really fell silent. `sdk.audio.info().native_voices` counts only
  the mod's native-mixer WAVs, not AEMS voices of a post; `F7_release` still checks the handle. A per-handle voice count
  (e.g. `sdk.audio.handle(key).voices`) would need an engine API.
Muted run 2026-10-04 (`20261004_170003-on`, 5 s / 3 s steps): 41 ok, 1 fail; the fail is `tags_after_resubscribe`
(the synthetic ollie of that step ended in a grind, FS 5-0: no pop row), a flaky ollie check, as in earlier runs.

Found by it (2026-10-04): the event tags (`pop`, `land`) are computed only when a mod subscribes while the native audio
already runs (`AudioApi::subscribe`; refreshed otherwise only by a `player` tuning write). A mod that subscribes in
`on_load` (before the native audio starts) keeps the empty default for the whole session: pops arrive untagged and a
Skate_Collisions Splice with id 0 is tagged `land`. Rules are not affected (they read the tags from the native player).
Open: refresh the tags when the native audio starts / restarts (engine fix, not done here).

### M9. Open questions

1. **Own instances for NPC skaters** (L3): their host runs a whole skater's components and its own grain bed per
   Player-slot instance (the runtime has one NPC bed); left in retail's Player slot. Wanted?
2. **Own-instance default for mod cars / peds**: answered 2026-10-04, the user: "yes I agree it should match"
   (the mod emitters, own by default; before that: "modding isn't in retails, so 'not like retail' isn't a thing.
   Yes default to seperate slots."). Own instances are the default for a mod's cars and peds, `slots = 'shared'`
   opts into retail's pools; a full private pool lets the nearest play and the rest wait (M3).
3. **The restart fallback**: a mod replacing the MixMap file or a grain recording still restarts the sound. The bed
   could be rebuilt in place (a cut of the rolling only); worth it?
4. **Swapped content restarts its own sounds**: a replaced bank's held posts continue with new instances (their
   programs start again); a replaced Splice tree's sounding sounds stop; a speech change stops the current lines and
   resets the speech managers' timers.
5. **Seed scope**: one seed sets every audio generator; per-generator seeds (only the world's) if a test needs them.
6. **Hot reload and Bevy clips**: a changed WAV the script plays through a Bevy voice (`native = false`) still
   reloads the whole mod.

---

# Design and research (2026-10-03 / 04)

The plan this was built from. Sections 0–3 are the design (2026-10-03); section 4 holds the research for the chosen
scope: prior art, the engine facts each phase needed (file:line on the branch at a4ec831), the open questions and the
refined plan. Line numbers drift; each reference also names the item.

---

## 0. Design in one paragraph

All retail audio data already flows through one place: `Library` (`game_audio/library.rs`), which reads
`audio_manifest.json` (banks, samples, AEMS projects / banks / MixMap / Splice, grains, wheels, emitters, location
sets, zones, crossfades, regions, player / grain / bus / world tuning, speech). Most of the moddability pass is
therefore **one content overlay at the `Library` level**: mods ship manifest-shaped fragments (`audio.json`) plus
files, merged mod first and then the install, by retail identity. With no audio mod active the overlay is empty and
`Library` is the same value it is today, so retail stays the default and byte-identical. Everything hard-coded
that is *data* (map → `.ems` files, beds, crossfade banks, bank lists, keep-list) moves into the manifest (setup
export) or into a code table that the overlay can extend. Behaviour that data can't express gets a small **runtime
API**, the same for Rust engine systems and Lua: retail posts / globals / MixMap reads, tuning overrides, emitters
and reverb zones as world-audio objects, mod WAVs routed through the native mixer, and audio events (observe in Lua,
suppress / replace through declarative native rules). The API follows the SDK conventions: commands validated
before allocation, keys and overrides owned by the calling mod, first owner wins, `nil` restores, per-mod and
global limits, capabilities, and cleanup on disable / reload / failure.

---

## 1. Inventory

Columns: **today** = what a mod can do now; **hard-coded** = what blocks it (file:line); **wanted** = what modders
would plausibly want.

Mod surface today (all features):
- `sdk.audio.preload / play / update / stop / stop_all`: the mod's own PCM16 WAVs as **Bevy voices outside the
  native mixer** (`skate-game/src/modding/audio.rs:1–40`). Limits 32 voices / 32 clips / 32 MiB per mod, 128 / 128 /
  128 MiB in all (`audio.rs:10–15`). They get no reverb, buses, ducking or retail distance curves; master volume and
  `--mute` apply via `GlobalVolume`.
- `sdk.world_audio.spawn / update / event / remove / read / info` (capability `world_audio` = 1,
  `skate-mods/src/world_audio.rs`, `modding/world_audio.rs`): traffic, peds, skaters (lite or ghost), horn / alarm /
  speech events. 48 objects per mod, 128 in all.
- **Gap found (fixed in R0, 2026-10-04):** `sdk/skate.lua` (the language-server declarations) had **no `sdk.audio.*` entries**; they exist
  only in `skate-mods/src/api.lua:266–288`. `sdk.audio` also has no capability entry (only `sdk.audio.version = 1`).
  `sdk/AGENTS.md`, `GENERAL_API.md` and `ENGINE_API.md` don't mention audio.
- No mod can replace or add retail content, post retail sounds, read or write globals or the MixMap, change tuning,
  or see audio events.

### 1.1 Player audio

| feature | today | hard-coded | wanted |
|---|---|---|---|
| Player components (grind, sense of speed, foot drag, skid, squeaks, seams, rolling layers, rattle, board slide, tricks, treatment, footsteps, clothing, body slide) | nothing | bank lists `player_audio.rs:559–572` (`BANKS` = `components::BANKS` `skate-audio/src/player/components.rs:67`, `ROLLING_/RATTLE_/SLIDE_/TRICKS_/TREATMENT_BANKS`, `SPLICE_BANKS`, `WHEEL_STREAMS`); class names are `&'static str` constants (`rolling.rs:27–29`, `seams.rs:46`, `treatment.rs:20–21`, `footsteps.rs:44–45`, `Command::Post { class: &'static str }` `components.rs:61`); banks bound once in `Native::load_player_banks` / `load_optional_player_banks` (`native.rs:369–459`) | replace a grind / pop / landing / trick sound (sample or bank); change grind surfaces or per-material sounds; silence or layer a component |
| Splice (pops, landings, touchdowns, foot taps, scuffs, collisions) | nothing | `SPLICE_BANKS` (`player_audio.rs:570`); Splice trees only from `manifest.aems.splice` (`library.rs:1035`) | replace members (samples) or a whole patch tree; custom pop sound |
| Player tuning (surface table, jitter, seams, grind surfaces, landing materials, audio tricks, collision materials, rolling, tricks, treatment; contacts / wheels / footsteps / clothing tuning) | nothing | read **once** at start: `PlayerAudio::new(library.player_tuning(), true)` (`native.rs:~284`), `contacts_tuning`, `footstep_materials` (`native.rs:~413–416`); `PlayerTuning` has no serde (`skate-audio/src/player/tuning.rs:98–99`) | tweak per-material sounds, grind surface levels, trick sound ids; read values for HUD / debug mods |
| Wheel spins (SFXObj_Wheels) | nothing | `WHEEL_STREAMS` (`player_audio.rs:572`), loaded once | replace the spin recordings |
| Grain bed (granular rolling) | nothing | member lists `grain_bed.rs:52–70` (`MEMBERS`, `SOFT_MEMBERS`, `ROCKET`); tag → member `grain_for` is a code `match` (`grain_bed.rs:~76–97`) although the manifest has `grain_player.surface_map` (`library.rs:994`); bed built once (`native.rs:~282`) | replace a surface's recording (e.g. custom wood); map a new material to a member; tweak grain tuning |
| Trick → audio trick ids | nothing | `PlayerTuning::audio_tricks / _2` (setup export) | give a mod trick its own sound |

### 1.2 Mixer, buses, MixMap

| feature | today | hard-coded | wanted |
|---|---|---|---|
| Buses / reverb (env network presets, eEQChain, FlangeSub, FootStep SubMix) | nothing | presets / EQ / flange read once from `manifest.bus_tuning` in `Native::start_with` (`native.rs:~305–322`); SubMix always on | change reverb presets (a cave mod), EQ; route mod sounds through them |
| Reverb zones (`.ems` eVolumeType 5) | nothing | `emitters.rs:132` `zone_records` from `ems_files(map)` (`emitters.rs:55–74`) | add / move reverb zones (custom maps, mod buildings) |
| MixMap (curves, controllers, instance layout) | nothing | the `.mxb` from the install only (`native.rs:~263`); layout `RETAIL_INSTANCES` (`skate-audio/src/mixmap/mod.rs:31`), `WorldInstances::RETAIL / MORE_AUDIBLE` (`native.rs:227–240`), `PLAYER_INSTANCES` / `AUDIO_RADIUS` (`world/skaters.rs:75–78`), `TRAFFIC_ / PEDESTRIAN_INSTANCES` (`world/keys.rs:9–10`) | read outputs (level / distance of a car) for HUDs; replace the whole `.mxb` (expert); more instances (already a user setting) |
| Volume groups / categories | menu only | `AudioSettings` (`game_audio/mod.rs`), `GROUP_WORLD / GROUP_PLAYER` (`skate-audio/src/mixer.rs:293–294`) | duck the world or player group (music / cinematic mods) |
| Host clock | n/a | `MIX_STEP`, `MAX_STEPS_PER_FRAME = 4` (`native.rs:42, 50`) | **nothing; engine-only by design** (the console cadence) |

### 1.3 World: emitters, location sets, ambience

| feature | today | hard-coded | wanted |
|---|---|---|---|
| World emitters (`.ems` type 1 via `c_emitter`) | nothing (mods use Bevy voices instead) | map → files `ems_file` / `ems_files` (`emitters.rs:35–74`, the 10 retail maps only; a custom map gets **no** emitters); records only from `manifest.emitters`; `MAX_ACTIVE = 5` (`emitters.rs:32`) = the MixMap Emitter instances (`native.rs:94` `EMITTER_STATES`) | add / move / remove emitters; emitters for custom maps; a positional retail sound (siren, fountain) from a mod |
| Location sets (random distant one-shots) | nothing | `LOADED = 2`, `PAN_DISTANCE` (`random_sets.rs:24–26`); measured layers per bank `random_programs.rs:7` (interim, Bevy voices; a bank without a row is **silent**, `random_sets.rs:41–43`); district = map stem for the region lookup; `SKATE_AUDIO_SET` env (`random_sets.rs:126`) | change a set's sounds / weights / intervals; new sets for custom maps; force a set from a mod (instead of the env var) |
| Zone ambience + crossfades | nothing | `BEDS` map → bed fallback (`ambience.rs:27–43`), `crossfade_bank` district table (`ambience.rs:45–52`), `BED_BASE` (`ambience.rs:22`), measured crossfade layers `crossfade_groups.rs:7` (interim, Bevy voices) | replace beds; new zones / beds for custom maps; change fades |
| Regions (world-painter `audio_emitters` / `audio_ambience` tiles) | nothing | only from `manifest.regions[district]` (`library.rs:931`) | regions for custom maps |
| Bank unload on map change | n/a | keep-list in `Native::unload_map_banks` (`native.rs:471–482`: `emitter_utility`, `Common`, the player banks) | a mod bank that must survive a map change |

### 1.4 World / NPC sources and speech

| feature | today | hard-coded | wanted |
|---|---|---|---|
| Traffic (engine, horn, skid, alarm) | spawn / update / event (`world_audio` 1) | banks `TRAFFIC_BANKS` (`skate-audio/src/world/mod.rs:69–81`); classes `traffic.rs:28–31`; list radius `TRAFFIC_LIST_RADIUS = 40` (`world_sources.rs:339`) | a custom engine sound (new bank bound to `TRAFFIC_CAR` with its own patch); tweak `aud_traffic_engine` records |
| Peds (footsteps, body fall, tazer) | spawn / update | `PED_BANKS` (`world/mod.rs:82`), classes `peds.rs:27–31` (`livingword_footstep`, `sk8_foley` step ids), `PED_LIST_RADIUS = 50` (`world_sources.rs:342`), `FOOTSTEP_PEDS = 3`, `PED_FAR_THRESHOLD = 20` (`world_bridge.rs:33–35`); ped footstep tuning read once at the first owner (`world_sources.rs:523`) | new shoe sounds; tweak footstep tuning |
| NPC skaters | spawn lite / ghost | 1 instance within 30 m (retail); the more-audible setting (start-time only) | (covered by world_audio) |
| Speech (ped lines, NPC bail grunt) | `event(key,'speech')` triggers lines | index / rules / takes from setup only (`library.speech`, `world_speech.rs:190, 361`); manager tuning `SpeechManager::new(library.world_tuning().speech_tuning())` (`world_speech.rs:360`); `STREAMS = 2`, `QUEUE = 16` (`speech_player.rs:44–46`); `BAIL_GRUNT_EVENT = 8206`, `KEEP_TAKES = 24` (`world_speech.rs:39–41`) | add lines / takes to a voice and event; new voices; tweak event probability / timers |
| World RNG | n/a | `Lcg(0x5EED)` / `Lcg(0x5EEC)` (`world_sources.rs:308`, `world_speech.rs:113`) | seed for reproducible mod tests (minor) |
| Env-only switches | n/a | `SKATE_AEMS_WORLD`, `SKATE_AEMS_WORLD_PREFETCH` (`world_sources.rs:214–221`), `SKATE_AEMS_NPC_SKATERS` (`npc_skaters.rs:50`), `SKATE_AUDIO_MORE_AUDIBLE` (`mod.rs`), `SKATE_AUDIO_SET`; dev: `SKATE_AUDIO_TRACE / TIMING / STATE_LOG(S)` | `SKATE_AUDIO_SET` as an API (debug mods); the rest stay dev switches (not mod-facing) |

### 1.5 Cross-cutting

- **No event access:** posts happen at four choke points a hook can tap without touching retail logic:
  `PlayerAudio::apply` (`player_audio.rs:~259–292`, every component post / redeliver / release with its `Slot`
  and class), `SpliceAccess::start` (`skate-audio/src/runtime.rs:267`, every Splice sound: bank + id), the
  world host's `WorldCommand` apply (`world/mod.rs:60–63`, owner + slot + class), and the speech host
  (`world_speech.rs`). Retail edges (pop, land, grind on / off, bail) are detected inside the components, so
  hooks should be keyed on these retail identities, not re-detected from physics.
- **No retail post / global access:** `Runtime::post / redeliver / release`, `Evaluator::class_id / global_id /
  set_global / global` exist (`runtime.rs:92–109, 249–257`; `eval/mod.rs:297, 458, 471`) but nothing outside
  `game_audio` reaches them.
- **Mod WAVs bypass the native mixer**, although the mixer can already open direct voices on a bank's samples
  with a route (`Mixer::add_bank`, `open_direct`, `open_routed`, `set_bank_sample` `mixer.rs:326–469`).
- **Read once at start:** AEMS projects, MixMap, bus tuning, player tuning, grain bed, player banks, Splice,
  wheel streams (`Native::start_with`). Content that changes after start needs a reload path.
- **Data path:** `Library::load` (`library.rs:861`) and `Library::bank_source` (`library.rs:1051`) read only the
  install. This is the single choke point for a content overlay.

---

## 2. Proposed surface

### 2.1 Content overlay (capability `audio_content` = 1)

**Where:** a mod ships `audio.json` at its root (not a `mod.json` key: `Manifest` is `deny_unknown_fields`
(`skate-mods/src/schema.rs`), so a new key would make the mod fail on older engines; a separate file is ignored
by them). Files are mod-relative, read with `read_bounded` like everything else.

**Shape:** sections with the same names and row shapes as `audio_manifest.json`, so the overlay is "a manifest
fragment" and setup's own exporters double as authoring tools:

```jsonc
{
  "version": 1,
  "replace": {
    "samples":   { "Skate_Collisions": { "1187": "audio/splash.wav" } },          // bank + sample index
    "banks":     { "C04_taxi01": { "abk": "audio/C04_taxi01.abk", "samples": ["audio/taxi_0.wav", "..."] } },
    "splice":    { "Skate_Collisions": "audio/Skate_Collisions.splc" },           // whole patch tree (expert)
    "grains":    { "wood_ramp_hard": { "file": "audio/wood.wav", "grain": "audio/wood.grn" } },
    "wheels":    { "Whls_spins_Jump_1": "audio/spin.wav" },
    "ambience":  { "04_dt_main": "audio/dt_main.wav" },
    "speech":    { "livingworld": { "takes": { "501_59_busm1_Warn_n": { "3": "audio/warn3.wav" } } } },
    "mixmap":    "audio/MixMapSK8.mxb"                                            // whole file (expert)
  },
  "add": {
    "banks":     { "MOD_siren": { "abk": "audio/MOD_siren.abk", "samples": ["audio/s0.wav"] } },
    "speech":    { "livingworld": { "lines": [ { "voice": 59, "event": "Warn", "takes": ["audio/w.wav"] } ] } },
    "random_sets": { "e_dwtn_spillway_brewery": { "entries": [ { "bank": "MOD_siren", "weight": 3 } ] } },
    "location_programs": { "MOD_siren": [ { "delay": 0, "sample": "shuffle", "level": 1 } ] }
  },
  "tuning": {                                    // field merges on setup's tuning sections; validated, typed
    "player":  { "grind": { "3": { "level": 0.8 } } },
    "world":   { "traffic_engine": { "c04_taxi01": { "...": 0 } }, "speech_tuning": { "1": { "53": { "probability": 0.5 } } } },
    "bus":     { "reverb": { "<preset key>": { "...": 0 } } }
  },
  "maps": { "MyCustomMap": { "...": "see 2.2" } }
}
```

Rules:
- **Resolution by retail identity, mod first, then the install.** Identities: bank stem; bank + sample index; Splice
  tree stem; grain member; wheel stream; ambience bed; speech archive + clip + take; set / zone key (hex) or name;
  emitter file + record index; tuning section + record name + field.
- **Order between mods:** packages in mod-id order (the manager's `BTreeMap`), **first owner wins**, and a conflict
  is reported in the mod menu diagnostics (the same rule as graph gates, rig parts and input overrides:
  "owned by another mod"). User decision Q4.
- **Validation before allocation** (`check_mod` too): schema with `deny_unknown_fields`; paths via
  `valid_audio_path`-style checks; WAVs through `canonical_pcm_wav` (PCM16, 1–2 ch, 8–48 kHz). The duration cap is
  30 s for samples, and longer only for `ambience` / `speech` takes, under the byte budget. `.abk` through
  `formats::Bank` parse (it already rejects opcodes ≥ 40 and bad block walks); `.splc` / `.mxb` / grains through their
  parsers. Budget per mod: 64 MiB of decoded PCM, 256 MiB in all; bounded counts (e.g. 512 sample replacements,
  64 banks per mod). A bad entry is skipped with a diagnostic and the retail sound stays.
- **A new bank needs a class to post to.** Phase R binds mod banks to an **existing** retail class (a new engine
  sound bound to `TRAFFIC_CAR` with its own patch number, a location-set bank posted by the set scheduler, an
  emitter bank posted through `c_emitter`). New Csis classes (a mod `.csi` project) come later (L4).
- **Interim location / crossfade programs:** a mod bank in a location set has no measured row in
  `random_programs.rs`, so `add.location_programs` supplies one. Without that row a mod bank gets a single shuffle
  layer at level 1. Retail banks are unchanged. These tables go away when the sets move onto the native evaluator
  (a parity item, not this pass).
- **When it applies (Q1):** at mod enable / disable / reload the overlay set changes. The `Library` is rebuilt
  (install manifest + overlays), and the native runtime **restarts at the next safe point**: `Native` is dropped and
  `start_with` runs again, the world hosts take the map-epoch reset, and the location / zone state is rebuilt. This
  is a short sound cut at enable / disable only. A bank-level hot swap (unload + `ensure_bank` + epoch, the map-change
  path) is an optimisation for later (L1). With no audio-content mod the rebuild never happens.

**Rust (engine side):** `AudioContent` resource: `register(owner, Overlay)` / `unregister(owner)`, with
`Overlay` the same parsed type the mod file becomes. `Library::load_with(asset_root, &[Overlay])` merges it, and
`Library::source(identity) -> (root, file)` replaces `root.join(file)` (a `FileRef { root: Install | Mod(id), path }`).
A future DLC or Skate 2 content pack (todos `dlc-support.md`, `skate2-support.md`) can use the same overlay.

### 2.2 Data-driven map tables (custom maps get audio)

Move the per-map code tables into data:

| today (code) | new home |
|---|---|
| `ems_file` / `ems_files` (`emitters.rs:35–74`) | manifest `maps.<stem>.ems` from setup: retail's own list, the map database entry `F4917ACACAFAF913` field `65FA976EF23A314E`, which the code comment already names. The code table stays only as a test oracle (`every_map_has_its_emitter_file`). |
| `BEDS` fallback (`ambience.rs:27`) | `maps.<stem>.fallback_bed`. It is not retail data, so it stays only for installs without zones. |
| `crossfade_bank` (`ambience.rs:45`) | `maps.<stem>.crossfade_bank` (setup export) |
| district = map stem for regions | `maps.<stem>.district` (default: the stem) |

A map's audio = `MapAudio { ems: [files], emitters: [records], reverb_zones, regions, zones, random_sets,
crossfade_bank, fallback_bed }`, assembled in this order:
1. the install (retail maps);
2. a **sidecar** `<map>.audio.json` next to a custom `.skate` file (map authors). Q7;
3. mod overlays `maps.<stem>` (first owner wins per section).

`CurrentMap` changes already rebuild emitter / zone / set state; they now read `MapAudio` instead of the code
tables. **Rust:** a `MapAudio` resource, inserted by the map loader. An engine map importer (Skate 2, DLC) fills it
directly.

### 2.3 Runtime API (Lua and Rust)

All new Lua functions go into `api.lua` (wrappers that `submit{kind=…}`) and `sdk/skate.lua` (declarations), with
validated `Command` variants in `vm.rs` and handlers in `skate-game/src/modding/audio_*.rs`. Each has a Rust
equivalent, and the mod command handler calls it, as `world_audio` does.

**(a) Retail posts, globals, catalog** (capability `audio` = 2)
```lua
local h = sdk.audio.post('emit1', 'c_emitter', {words})  -- key-scoped handle; words: ≤ 32 i32
sdk.audio.redeliver('emit1', {words})
sdk.audio.release('emit1')
sdk.audio.set_global('g_name', value)    -- nil restores the value seen before the first write
local v = sdk.audio.global('g_name')     -- snapshot read
local o = sdk.audio.mixmap(slot_key, output) -- read-only MixMap output (level, filter Hz, pitch) of the last pass
sdk.engine.inspect('audio_catalog', 'audio') -- classes, functions, globals, loaded banks, map ems files, sets, zones, tuning names
```
- Limits: 32 held handles per mod, 16 posts per frame per mod, 128 handles in all. Unknown class / global →
  command error (so `sdk.commands.request` reports it). Handles are released on remove / disable / reload / failure
  and on map change (the epoch reset: the handle reads `released` and the mod re-posts).
- Globals: first owner wins; originals are restored on disable.
- **Parity note:** a mod post runs the bank's program, which draws from the evaluator's shared RNG (as every retail
  post does). With a mod posting, the retail random sequence differs from the no-mod run. This is expected and
  documented; with no mod nothing changes.
- **Rust:** `AudioPosts` system param (`post(owner, class, &words) -> AudioHandle`, `redeliver`, `release`),
  `AudioGlobals`, `MixMapReadback` resource (the last pass's outputs per key), `AudioCatalog`.

**(b) Tuning read / write** (capability `audio_tuning` = 1)
```lua
local t = sdk.audio.tuning('world.traffic_engine', 'c04_taxi01')    -- table or nil
sdk.audio.set_tuning('world.traffic_engine', 'c04_taxi01', {field=v}) -- merge; nil restores retail
```
- Domains, each a typed serde struct with `deny_unknown_fields` and finite / range checks:
  - `player.surface`, `player.grind`, `player.rolling`, `player.tricks`, `player.treatment`, `player.collision`,
    `player.contacts`, `player.wheels`, `player.footsteps`, `player.clothing`;
  - `grain.surface`;
  - `world.traffic_engine`, `world.ped_footsteps`, `world.speech_event`, `world.ped_model`, `world.traffic_model`;
  - `bus.reverb`, `bus.eq`, `bus.flange`.
- Applied between passes (no restart): `PlayerAudio.tuning` and the sub-tunings are fields today. The world host's
  `ped_tuning` is reset to re-read, the speech manager is rebuilt, and bus presets are swapped under the runtime lock.
- **Not tunable:** the MixMap's shape (instance layout, 30 m / 40 m / 50 m pools, 5 emitter states). That needs a
  restart and a deliberate non-retail option (as "more audible" is). Q3 covers the emitter pool.
- Static tuning changes can also ship in `audio.json` `tuning` (2.1). The runtime call is for dynamic mods.
- **Rust:** `AudioTuning` resource: `get(domain, name)`, `set_override(owner, domain, name, patch)`,
  `clear(owner)`.

**(c) Emitters and reverb zones as world-audio objects** (`world_audio` = 2)

These are two new kinds in the existing lifecycle, so the keys, limits, parking, `read` and cleanup come for free:
```lua
sdk.world_audio.spawn('fountain', 'emitter', {bank='Water_fountain', patch=3, position=p,
                       extent={6,6,6}, core=1.5, volume=0.8, falloff='squared'|'linear'|'flat', body=nil})
sdk.world_audio.spawn('cave', 'reverb_zone', {preset='<aud_reverb key or name>', position=p, extent=e, forward=f})
```
- An `emitter` is an `.ems` type-1 record added to the map's live list: the same reach test (sphere / ellipsoid,
  inner core), retail falloff curve, `c_emitter` post with MixMap Emitter payload, release on leave
  (`emitters.rs` `update`). It goes through the reverb, buses and ducking like retail emitters. A `reverb_zone` joins
  `ReverbZones` in reach order (eVolumeType 5).
- Emitter pool (Q3): **default = share retail's 5 Emitter states** (retail's rule: first reached is served first).
  The option is extra MixMap Emitter instances only while a mod has emitters; instances 0–4 are unchanged.
- **Rust:** components `WorldEmitter { bank, patch, extent, core, volume, falloff }` and `ReverbZoneVolume
  { preset, extent }` on an entity with a `GlobalTransform`. The map's own records are spawned the same way at map
  load (or kept as data; the bridge merges both lists).

**(d) Mod WAVs through the native mixer** (`audio` = 2)

This keeps the existing commands working. New optional `audio_play` fields (additive; an older engine rejects them
because the options are `deny_unknown_fields`, hence the capability bump):
```lua
sdk.audio.play('beep', {path='a.wav', position=p, native=true, group='world'|'player',
                        falloff={radius=30, curve='squared'}, reverb=true})
```
- A mod's preloaded clips become one **mod bank** per mod in the runtime (`Mixer::add_bank` with headers built from
  the WAV). Voices open with `open_routed` on the SFX route with the env tap (reverb by the current zone), are
  panned by the listener azimuth (`native::azimuth`) and get the chosen retail falloff curve. Pitch is the
  resampler's (≤ 4×, as validated already). Master / category volume and `--mute` apply as for retail sounds.
- Default stays the Bevy voice (`native=false`), so existing mods sound unchanged. Q2: flip the default later?
- The same limits as today (voices / clips / bytes per mod and total) are counted across both paths.
- **Rust:** `ModVoices` on the runtime: `open(owner, clip, params) / set / stop`. Engine UI sounds can use it too.

**(e) Audio events and rules** (capability `audio_events` = 1)

- **Observe (Lua, one frame late):** the choke points (§1.5) append compact rows to a per-frame buffer, **built only
  while some mod subscribes**:
  - fields: `kind` (`post`, `release`, `splice`, `speech`, `zone`, `emitter_enter` / `leave`, `world_claim` /
    `release`), `source` (`player` / `world` / `npc`), `class` or `bank`, `id` / `patch`, `slot`, `owner` (world key
    if mod-owned), `position` if known;
  - **named tags** from a small native table on retail identities: `pop`, `land`, `grind_start`, `grind_end`,
    `bail`, `splash`, `footstep`, `seam`, `trick`, `horn`, `alarm`, `speech_request`, `zone_change`.
  - Delivered as `sdk.snapshot.audio_events` (array, ≤ 256 per frame, truncation flagged) plus an optional
    `on_audio_event(event)` callback per row (≤ 64 per frame per mod).
  - `sdk.audio.subscribe{tags={...}}` limits what a mod pays for.
- **Suppress / replace / layer (native, same frame):** Lua cannot run inside the audio pass, so changes are
  **declarative rules**, evaluated at the choke points:
  ```lua
  sdk.audio.rule('quiet_pop', {match={tag='pop'}, action='suppress'})
  sdk.audio.rule('my_pop',    {match={tag='pop'}, action='replace', play={path='pop.wav', native=true}})
  sdk.audio.rule('layer',     {match={class='TRAFFIC_HORN'}, action='layer', post={class='c_emitter', words={...}}})
  sdk.audio.rule('quiet_pop', nil)  -- remove
  ```
  32 rules per mod. First owner wins per match key. Rules are removed on disable. With no rules the cost is one
  `is_empty()` branch per choke point.
- **Rust:** Bevy messages `AudioEvent` (same rows; engine systems read them with a `MessageReader`) and an
  `AudioRules` resource (`insert(owner, key, Rule)`, `clear(owner)`).

**(f) Small additions**
- `sdk.audio.set_group_volume('world'|'player'|'ambience', 0..1)`: a multiplier on top of the menu values, first owner
  wins, restored on disable (ducking for music / cinematic mods). Rust: `AudioDucking`.
- `sdk.audio.force_location_set(name|nil)`: replaces the `SKATE_AUDIO_SET` env var as an API; the env var stays a
  dev override.
- `sdk.audio.info()`: native on / off, map epoch, loaded banks, overlay owners and conflicts, limits, layout.
- `sdk/skate.lua`: declare the existing `sdk.audio.*` (the gap in §1).

### 2.4 Capabilities, limits, cleanup (summary)

| capability | covers |
|---|---|
| `audio` = 2 | `sdk.audio.*`: v1 commands unchanged, plus native routing, post / release / global / mixmap, ducking, info |
| `audio_content` = 1 | `audio.json` overlay, map sidecars |
| `audio_tuning` = 1 | tuning read / write |
| `audio_events` = 1 | event rows, `on_audio_event`, rules |
| `world_audio` = 2 | adds `emitter` and `reverb_zone` kinds |

Cleanup on disable / reload / failure goes through the existing retire path (`modding/mod.rs:797–906`, beside
`audio::stop_owner` / `world_audio::clear_owner`): release handles, restore globals, drop tuning overrides and
rules, despawn emitters and zones, unregister the overlay (→ restart if it had content), release the mod bank.
Commands from a failed callback are dropped (existing rule). 128 commands per callback (existing).

---

## 3. Phases

Effort is in focused working days, including tests and docs. The ordering keeps the no-mod path identical at every
step.

### "Ready" for PR #32 (proposed; the user decides the cut, Q5)

| phase | content | effort | key risks |
|---|---|---|---|
| **R0 baseline + docs gap** | declare `sdk.audio.*` in `sdk/skate.lua`; `sdk.audio.info()`; `audio` catalog for `sdk.engine.inspect`; capture the e2e baseline (13 scenarios + whole sessions, row / fps300) for the byte-identical proofs | 0.5 | none |
| **R1 content overlay** | `audio.json` schema + validation (`check_mod` too); `Library::load_with` + `FileRef` roots; replace / add for samples, banks, Splice, grains, wheels, ambience, speech takes and lines, sets, zones, emitters, MixMap file, tuning sections; restart-on-change; mod banks kept across map changes (keep-list → data: banks owned by an overlay are kept); conflicts in diagnostics | 3 | the restart path (stream handover, the prefetch worker, the world epoch); speech index merge; budgets |
| **R2 map tables** | setup export of `maps.<stem>` (retail's map → `.ems` list, crossfade bank, district); `MapAudio` resource; sidecar `<map>.audio.json`; the code tables become test oracles | 1.5 | needs a setup refresh (audio group); must reproduce the code tables exactly (test) |
| **R3 posts / globals / mixmap / tuning** | 2.3 (a) + (b) + (f) | 2.5 | the shared-RNG note; tuning applied mid-session must not tear a pass (apply between passes) |
| **R4 emitters, reverb zones, native mod WAVs** | 2.3 (c) + (d) | 2.5 | the emitter pool decision; mod bank lifetime vs runtime restart; pan / falloff matching retail emitters |
| **R5 events + rules** | 2.3 (e) | 2.5 | choke-point cost (must be 0 without subscribers, proven); tag table correctness (each tag tested against a real post) |
| **R6 examples, docs, proofs** | example mods (below); doc 16 "Audio modding" (+ doc 11 / 15 / PULL-REQUESTS rows); `make-mod` skill; SDK docs | 1.5 | — |

Total ≈ 14 days. A smaller cut (Q5) is **R0 + R1 + R2 + R3(a) + R5 (observe only) + R6** (≈ 9 days): every
feature's *data* is moddable, retail sounds can be posted, and events can be seen. Tuning writes at run time, mod
emitters, native WAV routing and rules would follow.

### Later

All done (2026-10-04, branch `audio/moddability-2`): L1–L5 and L7 in section M, L6 in H.

- **L1** bank-level hot swap instead of the runtime restart. Done: M1.
- **L2** writable MixMap inputs (mod ducking through retail's controllers rather than a group multiplier). Done: M2.
- **L3** extra MixMap instances for mod objects beyond the more-audible setting. Done: M3 (cars, peds; the default
  for a mod's cars and peds since `world_audio` 4).
- **L4** mod Csis projects (new classes, functions, globals). Done: M4.
- **L5** a seedable world RNG for reproducible mod tests. Done: M5 (every audio generator).
- **L6** flip `native=true` as the default for mod WAVs (Q2). Done: H.
- **L7** hot reload of `audio.json` while the mod runs (the manager already fingerprints packages; changes then
  trigger R1's rebuild). Done: M6 (the script keeps running; the change is hot-swapped).

### Tests and proofs

- **No-mod identity:**
  - `Library::load_with(root, &[])` equals `Library::load(root)` (field-wise, data-gated);
  - the e2e bench byte-identical before / after each phase (R0 baseline vs each phase, 13 scenarios + sessions, row
    and fps300);
  - the oracle tests (evaluator, MixMap, DSP, grain, splice) unchanged;
  - R2: the exported `maps` table equals the old code tables for all 10 maps.
- **Choke-point cost:** with no subscribers / rules, the bench's per-pass timings match within noise and no
  allocation happens in the hook (skill optimisation §3b review).
- **Per feature, one mod-driven test** (headless, data-gated where needed):
  - sample replace → the voice reads the mod PCM;
  - bank replace / add → class binding and the patch plays;
  - Splice member replace; grain member replace; speech take add → the manager picks it;
  - set / zone / emitter overlay → the scheduler uses it;
  - custom-map sidecar → emitters and zones on a test map;
  - post / global / mixmap read; tuning set and restore (the output changes, and after `nil` it matches the
    baseline again);
  - emitter object audible within reach, released on leave; reverb zone raises the env send;
  - native WAV voice routed with the env tap;
  - each event tag fires on its real post; rule suppress / replace / layer.
- **Lifecycle:**
  - disable / reload / failing mod → the overlay is gone, the runtime is restarted, and the e2e output equals the
    no-mod baseline (the "removed mod restores retail" proof);
  - map change with mod handles (epoch);
  - two mods on one identity → first wins with a diagnostic.
- **Validation:** `check_mod` rejects bad `audio.json` (unknown field, path escape, bad WAV, oversize, unknown
  identity reported as a warning). `vm.rs` serde-boundary tests for every new command (valid / invalid / unknown
  field), as for `world_audio`.
- **Example mods** (dev-only under `mods/` unless the user says otherwise, Q8):
  - `audio-custom-pop`: sample replace + a rule (replace pop) + an event-driven HUD line;
  - `audio-louder-horns`: tuning + a bank add bound to `TRAFFIC_CAR`;
  - `audio-siren-emitter`: an emitter object + a reverb zone + `sdk.audio.post`;
  - a custom-map sidecar sample in the docs.

### Risks (overall)

- **Retail parity stays the default:** the overlay and rules are empty without mods, and every new path is gated on
  "some mod uses it". The byte-identical e2e proof is required per phase.
- **The runtime restart** is the riskiest new mechanism. It reuses the map-change epoch reset (already tested) but
  also restarts the stream and the bed; R1 needs a test that a restart with an empty overlay gives byte-identical
  output from that point on.
- **Mod posts shift the shared RNG:** documented, mod-only.
- **Content licensing:** overlays reference retail identities, not retail data. Mods ship their own audio, and
  nothing copied from the game goes into the repo. Example mods use self-made
  sounds.
- **Concurrent edits:** the files are the same as the ongoing world / speech work and the optimisation pass, so this
  runs after them (#32 first).

## 4. Research for this PR (2026-10-04)

File:line references are this branch (code base a4ec831).

### 4.1 Prior art

- **Upstream and forks:** there is no earlier mod content-override or audio-modding work upstream (PRs and issues up
  to #36) or in the active forks. The closest precedent in the engine is `custom_difficulty.rs`, which overlays
  user values onto collections by hash identity.
- **Other games** (design ideas only, from their public documentation):

| project | mechanism | what this design takes from it |
|---|---|---|
| Minecraft resource packs | `sounds.json`: packs merge per sound event; `"replace": true` drops lower packs' entries | `replace` / `add` sections merged per identity |
| Source engine | `maps/<map>_level_sounds.txt` overrides sound scripts for one map | a per-map sidecar for custom maps |
| Garry's Mod | the `EntityEmitSound` hook sees every sound; it can suppress or modify it | observe in Lua now; suppress / replace later, as declarative native rules (Lua can't run inside the audio pass) |
| Factorio | data stage vs control stage; changing mods needs a restart | data overlay ≠ run-time API; a restart on a content change is accepted practice |
| Bethesda games, GZDoom | records by stable id or logical name, load order decides, conflict reports | stable identities; the winner and the loser are reported |
| RimWorld | patch operations on single fields of XML defs | field-level tuning merges |
| FMOD / Wwise mods, FiveM | extra banks registered beside the game's; data by hashed names | mod banks bound to an existing class; 16-hex keys |

### 4.2 Engine facts per phase

**R1 content overlay**
- *Shape:* `Library` (`game_audio/library.rs`) holds the install manifest (`Manifest` :298–333: ambience, grains,
  wheels, banks = WAVs by S10A slot, emitters, random_sets, zones, crossfades, regions, `aems` (projects, banks,
  mixmap, splice), grain / player / bus / world tuning). Unknown sections are ignored. Versions 3..=5 are accepted
  (:15).
- *Readers:*
  - every file is read through `Library::load` (:837), `clip` (:860, Bevy voices, cache keyed by relative path),
    `read` (:1005), `wheels_pcm` (:832), `splice_bank` (:1011), `bank_pcm` (:1020) and `bank_source` (:1027 →
    `BankSource`, one root, decoded on the prefetch worker);
  - their consumers are `native::start_with` (native.rs:232–333), `ensure_bank` (:338), emitters, reverb zones,
    ambience, location sets, the grain bed and the world / NPC hosts.
- *Merge point:* `Library::load_with(asset_root, overlays)`: the install manifest, then the overlays in mod-id order,
  with the first owner of an identity winning. Each file becomes an absolute path from its root (install or mod).
  This touches three places:
  - `BankSource` needs per-file paths, since a retail bank with one replaced WAV mixes roots;
  - the clip cache key must include the root;
  - replaced AEMS samples need new S10A headers. Voices take frames, rate, channels and loop start from the `.abk`
    header (mixer.rs:129–174, 385–391; `Runtime::load_bank` runtime.rs:80–85), so a replacement WAV of another
    length or rate would play wrong. Splice banks and the wheel streams already build their headers from the PCM
    (splice/mod.rs:209–221).
- *Validation:*
  - `deny_unknown_fields` schema;
  - mod paths through `read_bounded` (skate-mods archive.rs:150);
  - WAVs through `canonical_pcm_wav` (skate-mods audio.rs:76–139: PCM16, 1–2 channels, 8–48 kHz, ≤ 30 s, ≤ 8 MiB).
    Ambience beds need a larger per-kind cap;
  - `.abk` / `.splc` / `.mxb` through the skate-audio parsers;
  - unknown identities are a warning, never an error.
- *Conflicts:*
  - The existing "owned by another mod" cases are run-time claims that fail the second mod (engine_access.rs:71,
    modding/mod.rs:130 / 943, player_physics.rs:26 / 567, attachment.rs:19).
  - Static overlay conflicts need a new persistent list that the mod menu shows. `Manager.diagnostics` is cleared on
    every package scan (skate-mods lib.rs:106).
- *Lifecycle:*
  - The overlay is package state: active while the mod runs.
  - It changes on start, disable, reload (a package fingerprint change also reloads, so editing a file hot-reloads)
    and failure (the `retired` list, modding/mod.rs:797ff).
  - It survives map changes: `clear_runtime` (mod.rs:734) clears only run-time state.
  - Several changes in one frame cause one restart.
- *Boot order:* `Library` loads at Startup and the native runtime at PostStartup, but mods are first scanned on frame
  1. Starting the native runtime after the first scan avoids a second start when an audio mod is on at boot.
- *Restart* (on a change of the active overlay set):
  - rebuild `Library`;
  - despawn the stream's player (the sink holds the runtime);
  - start a new `Native` and spawn a new stream.
  - Two details are required for correctness:
    - The new evaluator continues the old node counter (`next_node` starts at 1, eval/mod.rs:174), and `map_epoch`
      becomes old + 1 (a new `Native` starts at 0). The world and NPC hosts release their held nodes when the epoch
      changes (world_sources.rs:309, npc_skaters.rs:105). With a restarted counter those stale ids would release
      the new runtime's live posts (node 1 is the `c_emitter_utility` boot post). Without the epoch bump, hosts
      that saw epoch 0 would never reset.
    - The emitters, reverb zones, ambience and location sets rebuild only when `(map name, generation)` changes
      (emitters.rs:113 / 316, ambience.rs:108), so an audio-content generation joins that identity. Nodes held
      from the old runtime are forgotten, not released.
- *check_mod:* today it validates only `mod.json` and the Lua syntax (`validate_package`, archive.rs:124). It gains an
  `audio.json` pass (schema, paths, WAV / format parsing, budgets) and a summary. With an install path, it also
  reports unknown identities and conflicts with other mods.
- *Speech:* the speech index and takes are not on this branch yet (they come with #32's world speech). The speech
  overlay and the speech event site follow when that lands here.

**R2 map audio as data**
- Retail keeps it in the stock collections (`skater-collections.json`, present in every install):
  - class `F4917ACACAFAF913`: 11 district records plus `default`;
    - field `65FA976EF23A314E` = the `.ems` files, e.g. dist_university: music_ / sfx_ / reverb_ / speakers_ /
      crowds_university; each park one `sfx_` file; skateschool `skateschool.ems`;
    - field `33526BC9D1C36B4A` = the crossfade bank (only downtown, industrial, university).
  - The `world` rows point at that record through field `99D6E51C9E20A663`. Park variants (full / empty / tutorial)
    share their park's record. DLC rows point at the empty `default`.
  - A map file `<stem>.skate` comes from `DIST_<stem>` (install.py:263–284). World rows carry `WorldStream =
    DIST_<stem>`, and setup's spawn stage already joins them that way (map_starts.py:58–75).
- Two ways in:
  - (A) a setup export: a `maps` table in the audio manifest (`audio_export.convert`). This takes one audio-group
    refresh. Leave the manifest version at 5: the table is optional, and a version bump would make older engines
    refuse the whole manifest.
  - (B) a run-time lookup in the collections the game already loads (`skate_data::collections::Collections`). No
    refresh.
  - Either way the current code tables (`ems_file` / `ems_files` emitters.rs:35–74, `crossfade_bank`
    ambience.rs:45) stay as the test oracle for the 10 maps.
- Custom maps today:
  - `.skate` files in `<install>/maps` and `maps/private` (map_library.rs:10–29).
  - Audio keys everything on the file stem, so a custom map gets no emitters, zones, location sets, regions or
    crossfades. Surface audio (rolling, footsteps) works, since materials carry an audio surface id.
  - SKATE v12+ files have tagged extensions (`{tag, schema, payload}`, skate_map.rs:119; `WMET`, `RWCM` in use),
    so an embedded audio extension is an alternative to a sidecar.
  - Mods cannot ship maps.

**R3(a) posts, globals, MixMap read**
- *API:*
  - `Runtime::post / redeliver / release` (runtime.rs:92–114);
  - `Evaluator::class_id / function_id / global_id` (eval/mod.rs:249–262);
  - `post` always succeeds, even with no bound bank; `release` is harmless on a dead node;
  - `set_global` notifies subscribers only on a change (:458); `global` (:471).
- *Threading:*
  - The runtime is `Arc<Mutex<Runtime>>`. The audio thread renders one block per lock and the evaluator ticks
    there. The game thread posts between blocks.
  - The MixMap is game-thread state in `Native`, without a lock. Reads: `level / filter_hz / raw / pitch_4096`
    (mixmap/mod.rs:426–461).
  - Mod commands apply in `modding::apply`, which is unordered against `native::mixmap_frame`. Posts are therefore
    queued and drained at a fixed point of the host pass.
- *Shared RNG:* every program draws from one generator (eval/mod.rs:143). A mod post that draws shifts every later
  retail draw. This is expected and only happens while a mod posts.
- *`c_emitter`:* its payload is MixMap-derived and needs an emitter state (`Native::emitter_payload` native.rs:507).
  A raw post is possible but unpositioned; positional mod emitters are the later `world_audio` kinds.
- *Limits and cleanup:*
  - handles per mod;
  - released on retire (beside `world_audio::clear_owner`, modding/mod.rs:805 / 878 / 904) and on a map change
    (`clear_runtime`);
  - a handle is dead after a map change or a restart and is never released by its stale id;
  - globals: original saved at the first write, restored on retire.
- *MixMap view:* the snapshot copies only the controllers mods asked for. A world object's MixMap instance comes from
  `sdk.world_audio.read(key).instance`.

**R5 observe-only events**
- The post sites are:
  1. `PlayerAudio::apply` (player_audio.rs:261, under the runtime lock);
  2. Splice starts through `rt.splice_host()` (player_audio.rs:207 / 397, npc_skaters.rs:241 / 268). A game-side
     wrapper keeps skate-audio unchanged;
  3. the world host's apply (world_sources.rs:256);
  4. the NPC host's own apply (npc_skaters.rs:74);
  5. the world emitters' `c_emitter` post (emitters.rs:407), plus zone / set changes;
  6. speech, later (see R1).
- *Zero cost:* an optional sink that exists only while a running mod subscribes; each site checks it once. Proof: the
  e2e hashes and bench timings.
- *Delivery:* the mod snapshot is built once a frame (`snapshot_ro`, modding/mod.rs:458), unordered against the audio
  pass, so the rows are double-buffered and arrive one frame late. They are filtered per mod by its subscription,
  ≤ 256 a frame with a truncation flag.

**R6 examples and docs**
- The upstream Skyline mod commits self-made WAVs together with the `synthesize.py` that makes them. The examples
  can do the same.
- `*.abk` is gitignored repo-wide, so examples prefer sample replacement and binding to existing classes.

### 4.3 Open questions (options)

1. **Mod emitters vs retail's 5 emitter slots** (a later phase):
   - share them (retail's first-reached rule);
   - or add extra MixMap Emitter instances only while a mod has emitters (instances 0–4 unchanged).
2. **Retail map list:** a setup export (one audio refresh) or the run-time collections lookup (no refresh). Both
   reproduce today's tables.
3. **Custom-map audio:**
   - Options: a sidecar `<map>.audio.json` next to the `.skate`, an audio extension inside the `.skate`, or both.
   - Content either way: retail identities only (`.ems` files by name, emitter and reverb-zone records, simple box
     regions for zone ambience / location sets / reverb, crossfade bank). WAVs come from mods.
4. **Example mods:** dev-only, or one small example shipped under `sdk/examples/` with synthesized sounds.
5. **When an overlay is active:** while the mod runs (a Lua failure removes it), or while it is enabled.
6. **Boot order:** start the native audio after the first mod scan, or accept one restart at boot.
7. **Speech:** ship the overlay without speech lines first, and add them when #32's speech lands.
8. **check_mod depth:** skate-mods may depend on skate-audio (pure Rust) so `check_mod` parses banks, Splice trees and
   the MixMap.
9. **Mute / replace rules:** later; declarative native rules.

### 4.4 Refined plan

| step | content |
|---|---|
| R0 | `sdk.audio.*` declared in `sdk/skate.lua`, the `audio` capability, `GENERAL_API.md`. e2e identity reference: the branch's headless e2e bench (13 scenarios + 8 whole sessions, row and 300 fps modes) is byte-identical to the base commit's |
| R1a | overlay types and validation (skate-mods), `check_mod` pass |
| R1b | `Library::load_with`, per-file roots, clip cache key, header rebuild for replaced AEMS samples; test `load_with(root, &[]) == load(root)` |
| R1c | active-overlay tracking, conflict list in the mod menu |
| R1d | restart: continued node counter, epoch + 1, content generation in the map identities, stream respawn, prefetch shutdown, native start after the first scan; test: a restart with an empty overlay renders like a fresh run |
| R1e | overlay-owned banks survive map changes (the keep-list in `Native::unload_map_banks`) |
| R2 | `MapAudio` per map (collections or manifest), sidecar, code tables as the oracle |
| R3(a) | queued posts, epoch / restart-safe handles, globals restore, catalog, MixMap watch list |
| R5 | optional sink at the five sites, double buffer, tags tested against real posts |
| R6 | doc 16, SDK docs, `make-mod`, examples |

Every step keeps the no-mod e2e output byte-identical to the R0 reference.

## Decisions (2026-10-03)

- **Scope of this PR:** the smaller cut: R0 + R1 (content overlay) + R2 (map audio as data) + R3(a) (posts, globals, a read-only MixMap view) + R5 observe-only (audio events) + R6 (docs, example mods). Tuning writes, mod emitters and reverb zones, mod WAVs in the native mixer and mute / replace rules follow later.
- **Turning an audio mod on or off** restarts the native audio (a short cut, as on a map change).
- **Mod WAVs through the native mixer:** opt-in per sound (`native = true`); existing mods behave as before. (Superseded 2026-10-04: native is the default, section H.)
- **Two mods replacing the same sound:** the first by mod id wins, with a warning in the mod menu.
- **Open:** see §4.3 (mod emitters' slots, the retail map list's source, the custom-map audio format, example mods, when an overlay is active, boot order, speech timing, check_mod depth, mute / replace rules).
