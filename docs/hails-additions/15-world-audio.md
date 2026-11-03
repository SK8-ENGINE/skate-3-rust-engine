# Hooking up world audio (traffic, pedestrians, NPC skaters)

Part of PR #32 (branch `gameplay/audio`; doc [11](11-audio.md) has the ported audio itself).

## Problem

Skate 3's living world has its own sounds: traffic engines with Doppler, horns, skids and car
alarms; pedestrians' footsteps and speech; and the board sounds of an AI skater near the camera. The
native audio port (`crates/skate-audio`, `skate_audio::world`) already plays all of them the way
retail does. But the engine has no traffic, pedestrian or AI-skater system yet. Nothing upstream or
in any fork has one, and no issue asks for one (prior-work check, 2026-10-03). The sounds therefore
had no way in.

The goal (the user, 2026-10-03): build out everything the game engine can easily hook into once those
systems are added, in this PR, and moddable like the rest of the engine.

## The change in one paragraph

A future engine system adds **components** to its own entities: `TrafficAudio`, `PedAudio`,
`NpcSkaterAudio`, plus an optional `AudioVelocity`. It sends a few **messages**: `PedSpeechEvent`,
`VehicleHorn` and `VehicleAlarm`. A **bridge** turns these into the audio hosts' inputs every frame.
The hosts apply **retail's limits** to decide who is audible, and the bridge marks the audible
entities with `WorldAudioInstance`. Lua mods reach the same path through `sdk.world_audio`. A dev
mod, `mods/world-audio-test`, makes it all audible now.

## Engine surface (`crates/skate-game/src/world_audio.rs`)

Position and heading come from the entity's `GlobalTransform`: its +Z axis is the forward
direction, the game's convention, and retail's vehicle `+112` is likewise the world matrix's forward
row (recomp gap run G1). Velocity comes from the transform's change per frame unless the entity has
an `AudioVelocity`. Give one to anything that teleports or is a kinematic proxy, because Doppler and
the 3-D rates read it.

**Lifetime = the entity.** To publish, insert the component. To release, despawn the entity or
remove the component. Owner ids are `Entity::to_bits()`, so a reused index counts as a new owner.

### `TrafficAudio` (retail `SFXObj_TrafficEngine` / `TrafficHorn` / `TrafficSkids`)

| field | retail | meaning | who fills it | default |
|---|---|---|---|---|
| `engine` | vehicle record `+168` | the `aud_traffic_engine` record by name, **or the living-world model** (`taxi01`, `sedan02`, `suv`, `minivan01`, …), which setup maps through retail's attribute chain (entity → vehicle spec → record, G1; export `world_tuning.traffic_models`): sedans / hatchback → `c01_family01`, sports / muscle → `c03_sports01`, taxi / patrol → `c04_taxi01`, SUV / pickup / minivan → `c05_truck01`. The engine's patch override picks c06 / c07 / c08 itself. | the vehicle system, from the model | none (an unknown name is logged once and stays silent) |
| `speed` | `+148` | m/s | the vehicle system | the length of the velocity |
| `load` | `+144` | the driver's signed acceleration in m/s². G1 measured −15.6 in a hard stop, up to +3 pulling away, and 0 while cruising. It goes into the engine and skid words × 3000. | the driving AI | derived from the speed change |
| `horn` | `+156` | `None`, `Honk(1..=5)` or `Alarm` | the AI (honk decisions) | `None` |
| `skidding` | `+160` | the tyres skid | the AI (hard braking or a swerve) | false |

### `PedAudio` (retail `SFXObj_PedestrianSFX` / `PedestrianSpeech`, ped state S, G2)

| field | retail | meaning | default |
|---|---|---|---|
| `voice` | `S+84` | the model = the speech voice id 1–96 (41–96 living-world peds; a pro 1–29 or the special cast 30–38 speaks on the main-cast channel). Its `aud_characteristics` record (setup export `world_tuning.ped_models`) gives the shoe class, the security kind, the speech type / variant / gender words and the far threshold (G2) | none (no speech, the defaults below) |
| `shoe_class` | `S+132` | 1–5 (`None` = the model's). Most models are 2; no model uses 1, which is silent. | the model's, else 2 |
| `weight` | `[obj+28]+144` | 1–5. It changes per ped in retail (1 beyond about 12 m, 2–5 nearer) and its meaning is open. | 1 |
| `close_range` | `S+96 == 64` | the security guards' close-range footstep levels and their speech levels (`None` = the model's type bit) | the model's, else false |
| `feet_down` | `S+74` / `S+73` | the walk animation's foot plants (A, B) | up |
| `foot_materials` | `S+140` / `S+144` | the audio surface materials under the feet | 0 (retail's pavements read 0 in every line) |
| `footsteps_on` | `S+68` | footsteps on | retail's rule: the 3 nearest peds in the list |
| `speech_distance` | `S+148` / `S+156` | distance to the listener / the model's far threshold. Beyond the threshold the far `_f` lines are used. | the 3-D distance / the model's threshold (20 m for regular peds, 30 for pros) |
| `speech_value` | `S+136` | the state graph's speech value | 0. Send `PedSpeechEvent` rather than writing it. |
| `tazing` | `S+80` | the ped tazes (retail: its state graph's `TazeEntity` state): SFXObj_Tazer holds `c_tazer`, the zap burst | false. `PedTazerEvent` holds it for a time. |
| `body_fall` | `S+76` | the ped animation's `BodyFallType` channel: each change to a non-zero value starts one PedBodyFall sound (8, 9, other) | 0. `PedBodyFallEvent` sends one key. |

### `NpcSkaterAudio` (the MixMap Player slot's second instance)

| field | meaning | default |
|---|---|---|
| `list_order` | the skater list position (retail walks its list in order) | spawn order |
| `state` | this frame's `AudioState`. A skater simulated with the player's physics uses `game_audio::skate_events::skater_audio_state(physics, skater, &mut memory, dt)`, the same builder the local player uses (proved identical, below). Anything else uses `AudioState::rolling(&LiteSkater { .. })`: speed, wheels, materials, grind / air flags. With that fill, rolling, surfaces, seams, grinds and landings sound; tricks and foot / body foley stay silent. | none (not published) |
| `remote` | a remote multiplayer player (see below) | false |
| `voice` | the skater's speech voice: the AI skaters' models 89–96 (living-world channel) or a pro 1–29 / the special cast 30–38 (main-cast channel). Its bail grunt and its reactions say lines of it | none |
| `reactions` | the skater record's reaction bytes this frame (`SkaterReactions`: a slam seen and by whom, a second slam reaction, a trick seen and by whom, its own crash, the chase flag): its speech process (`skate_audio::world::skater_speech`) says `101_pos` / `104_slam` / `150_pro_pos` / `151_pro_slam` (pros) or `101_pos` / `404`–`405` / `105_spec_chase` (AI skaters), and `906_aislm` / `131_collide_object` on its crash | none. `NpcSkaterReactionEvent` raises one for a console frame. |
| `loose_board` | the conditioner's loose-board state (`+780`: 0, 1 upside down, 2 on its side; `rolling::loose_board`): the board slide holds while set | 0 (ghosts: from their log) |

### Messages, resources and the read-back

- `PedSpeechEvent { ped, value: SpeechValue }`: sets the ped's speech value, so PedestrianSpeech sees
  the change and requests a line. A repeat of the current value goes through 0 for one frame, so it
  speaks again. `SpeechValue::from_name` accepts the state-graph names (`DoWarning`, `LongCheer`, …),
  the short names `warn` / `cheer` / `slam` / `flee` / `knockdown` / `nearby`, and numbers.
  **`warn` is 53** (`SpeechValue::WARN`): the code's warn, which the manager maps to `501_warn`. The
  state graph's `DoWarning` (11) entry is commented out in retail ("moved to the code"), and 11 is
  also `LostInterestEndChase`, which maps to `605_chase_terminate`.
- `VehicleHorn { vehicle, kind, seconds }`: holds horn kind `kind` for the caller's time. The length
  is the AI's choice, not retail data.
- `VehicleAlarm { vehicle }`: retail's alarm, horn state 6 for **8 s**.
- `PedTazerEvent { ped, seconds }`: the ped zaps: `tazing` held for `seconds` (None = the state graph's
  `TazerCycTime`, setup `world_tuning.ped_objects.tazer_seconds`, 2.0 s).
- `PedBodyFallEvent { ped, kind }`: one `BodyFallType` key of a knock-down animation. Keys queue; each is on for
  one console frame with a frame of 0 after it, so repeated keys sound.
- `NpcSkaterReactionEvent { skater, reaction, by }`: an NPC skater reacts (`SkaterReaction::{Slam, SlamB, Trick,
  Crash, Chase}`, `by` = the other skater's model, 0 = the player); held for one console frame.
- `LivingWorldAudio { expected, photo_flag }`: set `expected` at map load when the system will publish (the
  world banks then decode on the prefetch worker). `photo_flag` is the game flag of the photographer's repeat
  (a ped holding speech value 29 repeats it every second; retail system byte `+912`, meaning not traced).
- `WorldAudioInstance { slot, instance }` (read-back): the bridge inserts it on the entities that
  hold a MixMap instance. Only those are audible. An engine system can use it to skip per-frame
  audio work for the others, such as an NPC's `AudioState`.
- `WorldAudioStats`: published and audible counts, the instance layout, and whether the
  "more audible" setting is on.

### Who is audible (retail's limits, applied by the hosts)

| pool | instances | rule | source |
|---|---|---|---|
| Traffic | 4 | the 4 nearest within **40 m**, measured horizontally | G1: every holder was among the 4 nearest in 311 of 311 holder-seconds; the list is cut at 39.994 m |
| Pedestrians | 15 | the 15 nearest within **50 m**; footsteps for the **3** nearest | G2: manager `sub_824F2890`; `+68 == (index < 3)` in 2,803 of 2,803 lines |
| NPC skater | 1 | the first in list order within **30 m** of the camera, held until it is 30 m away or more | `sub_824F1FB0` / `sub_824F8EF8` |

**Opt-in "more audible" setting (not retail; user decision 2026-10-03):** `settings/audio.json`
`"more_audible_world": true`, or `SKATE_AUDIO_MORE_AUDIBLE=1` for one run. The MixMap is then built
with 8 traffic, 24 pedestrian and 4 Player instances (3 NPC / remote skaters). The extra objects
get retail's lookups, curves and posts; only their number is not retail. Instance 0 of every slot
is unchanged. The setting is read at start. The runtime has one NPC grain bed, so only NPC instance
1 has granular rolling. Default: off (retail).

**Remote multiplayer players (not retail; user decision 2026-10-03):** another real player nearby
takes the NPC skater instance, ahead of the NPCs in the list order. Retail's online behaviour is
not traced. The remote state carries only the body and pose, so the bridge gives remote players a
lite state from their root transform. The material under them comes from the same stock line query
the wheel lines use.

**No traffic-light or crossing sounds:** Skate 3 has none (the user, 2026-10-03).

### Example: an engine system

```rust
use crate::world_audio::*;

// A traffic system spawning a taxi (heading = the entity's +Z):
commands.spawn((Transform::from_xyz(10.0, 0.0, 4.0), TrafficAudio::new("c04_taxi01")));

// Its driving AI each frame:
fn drive(mut cars: Query<(&mut Transform, &mut TrafficAudio, &Car)>) {
    for (mut t, mut audio, car) in &mut cars {
        t.translation = car.position;
        audio.load = Some(car.acceleration); // m/s², or None to derive it
        audio.skidding = car.acceleration < -8.0;
    }
}

// The AI honks at a skater in the lane, a car is hit:
honks.write(VehicleHorn { vehicle, kind: 2, seconds: 0.8 });
alarms.write(VehicleAlarm { vehicle });

// A ped system: walk animation foot plants, reactions from its state graph:
commands.spawn((transform, PedAudio { voice: Some(59), ..Default::default() })); // shoe class, kind, words: the model's
speech.write(PedSpeechEvent { ped, value: SpeechValue::WARN });

// An AI skater simulated with the player's physics, near the camera:
npc.state = Some(skate_events::skater_audio_state(&physics, &skater, &mut memory, dt));
```

## Mod surface (`sdk.world_audio`, API 2, capability `world_audio` = 1)

Mods publish through the same components: each key becomes an entity. The rules follow the existing
mod rules. Keys belong to the calling mod. The limits are **48 objects per mod and 128 in all** (raised from 16 / 64 the same day: the
test mod publishes 37 objects; retail's pools pick the audible few anyway, so the limits only
bound the per-frame work, a distance per object and one sort).
Options are validated before anything is allocated, and a field of another kind is an error. An
object that is not updated for **0.5 s is parked** (speed 0, feet up, horn off). Disabling,
reloading or a failing mod removes everything it published. The existing `sdk.audio.*` (the mod's
own WAVs) is unchanged.

```lua
sdk.world_audio.spawn('car1', 'traffic', {engine='c04_taxi01', position=p, heading=h})
sdk.world_audio.update('car1', {position=p, velocity=v, speed=12, load=-3, skidding=false})
sdk.world_audio.event('car1', 'horn', {kind=3, seconds=1.2})   -- or 'alarm' (8 s)
sdk.world_audio.spawn('taxi', 'traffic', {engine='c04_taxi01', body='chassis'})  -- a mod car opts in to a retail engine sound
sdk.world_audio.spawn('ped1', 'ped', {voice=59, shoe_class=3, position=p})
sdk.world_audio.update('ped1', {position=p, feet={true,false}})
sdk.world_audio.event('ped1', 'speech', {value='warn'})        -- name or number (49: a phone call)
sdk.world_audio.event('ped1', 'tazer')                         -- the zap burst (seconds = 2 by default)
sdk.world_audio.event('ped1', 'body_fall', {kind=9})           -- one knock-down key (8, 9, other)
sdk.world_audio.update('ped1', {tazing=true, photo_flag=true}) -- held tazing; the photographer's game flag
sdk.world_audio.update('npc1', {loose_board=1})                -- lite skater: the board slide
sdk.world_audio.event('npc1', 'reaction', {value='trick', by=0}) -- it saw the player's trick (slam, slam_b, trick, crash, chase)
sdk.world_audio.update('npc1', {voice=24})                      -- a pro's voice: the main-cast channel
sdk.world_audio.spawn('npc1', 'skater', {position=p, speed=6})  -- lite: rolling from these fields
sdk.world_audio.spawn('ghost', 'skater', {source='state_log:state_20261003_143434', from=30, seconds=20, position=p})
local r = sdk.world_audio.read('car1')   -- {kind='traffic', audible=true, instance=2, parked=false}
local info = sdk.world_audio.info()      -- {more_audible=false, instances={traffic=4, peds=15, skaters=1}, ...}
sdk.world_audio.remove('car1')
```

`body=` makes an object follow one of the mod's physics bodies (position, rotation, velocity). This
is how a mod car opts in to the retail traffic engine sound; that choice is the user's
(2026-10-03) and is not retail. A **ghost** replays one of the user's recorded audio state logs
(`SKATE_AUDIO_STATE_LOG` writes them) at full fidelity. It reads `logs/<name>.tsv` in the mod
folder, or `<name>.tsv` in the folder named by `SKATE_AUDIO_STATE_LOGS`. A log with any malformed
line is refused.

## The dev test publisher (`mods/world-audio-test/`)

The mod is dev-only: it should not ship upstream unless the user asks. It drives everything around
the spot where the mod starts:

- **16 cars** on four rectangular lanes, cycling through the nine engine records `c00`–`c08`. Each
  car accelerates at +2.5 m/s², cruises at 8–15 m/s for 3–8 s, then brakes: hard (−12 m/s², with
  skids) or soft (−4). One car honks every 10–15 s with kind 1–5. One taxi is parked, and **F8**
  fires its alarm.
- **20 peds** on back-and-forth lines: 16 walk (1.3 m/s), 3 jog (4) and 1 runs (8), so the three
  `sk8_foley` step ids play. A gait clock alternates the feet; it is dev-only, not retail. Each ped is
  one of retail's 35 living-world models (its voice), so its shoe class, kind and speech words are the
  model's. Speech (it plays when the speech decode is installed):
  - warn (53, `501_warn`) when you pass within 1.5 m at more than 3 m/s;
  - cheer (23, `101_pos`) on a landed trick within 10 m;
  - slam (25, `104_spec_slam`) on a bail within 10 m;
  - every 6–10 s the nearest ped shouts (2, `1901_shout`; dev-only chatter, not retail). The
    manager's own tuning still gates every line (the shout: 50 % and 30 s per voice), and nothing
    speaks in the first 30 s after the game starts (retail's timers start at 0).
- **A ghost NPC skater** replays the 20 s window of a recorded state log, 5 m beside the start, with
  voice 91 (an AI skater's): its Wheels streams, Clothing foley and bail grunt play as an NPC's.
  Settings: `ghost_log`, `ghost_from`.
- **Off by default** (`"enabled_by_default": false`, a new optional `mod.json` field): enable it in the
  mod menu, or run with `SKATE3_MODS_ENABLE=dev-world-audio-test`. While it publishes, the game logs one
  `WORLD_AUDIO cars N/audible M, peds N/M (footsteps K), skaters N/M, posts +P (npc +Q), speech lines +S`
  line per second.
- Debug **boxes**: green = holds an instance (audible), grey = not. An on-screen line shows the
  audible counts, the limits and the ghost's status. **F9** re-centres everything on the skater.

## Proofs and tests

- **The map-change bug (P0).** `unload_map_banks` (a map change) destroyed every instance of the 13
  world banks. `WorldHost` kept its dead nodes and "already posted" objects, so an owner that
  survived the change redelivered to a dead node and stayed silent. The fix:
  - `Native::map_epoch` is bumped by `unload_map_banks`;
  - on a change both hosts release every node, clear their pools, deactivate the 3DObjPos blocks,
    stop the ped Splice steps and the NPC bed, and drop their objects;
  - the evaluation count is `saturating_sub`;
  - the evaluation `dt` follows the MixMap cadence: 1/30 per console evaluation. The old 60 Hz
    mode, where `CONSOLE_DT` doubled it, was removed with the A/B switches the same day (doc 11).

  Test `world_owners_post_again_after_a_map_change` (data-gated): a taxi and a ped publish, the C04
  engine sounds, the map changes, and the same ids post afresh and sound again. Without the epoch
  reset the test fails ("the same owner sounds again after the map change"). The ghost test below
  checks the NPC host's reset the same way.
- **The per-skater builder** (`skater_audio_state`, `SkaterAudioMemory`) is a pure move of
  `observe`'s code. Test `audio_state_capture` (data-gated) drives 1,500 production physics steps
  headless (push, carves, two ollies, a powerslide, a roll-out) and runs `observe` after each. Its
  published samples are **identical line for line** to a capture taken before the refactor. On
  every step the builder, run on its own memory, also gives the same `AudioState` as `observe`.
- **The local player's output:** the e2e bench (13 scenarios and 4 whole sessions, row and fps300)
  is **byte-identical** before and after this work (`wa_base` vs `wa_new`). The state-log replay
  that e2e uses was moved, unchanged, into `game_audio/state_replay.rs` so the ghost can use it.
- Bridge unit tests (a minimal Bevy app):
  - components become owners;
  - the ped footsteps-on and far-threshold rules hold, and remote players come first;
  - the alarm, horn and speech messages work;
  - the read-back inserts and removes `WorldAudioInstance`;
  - despawning releases, and nothing happens without components.
- `a_ghost_claims_instance_1_releases_and_survives_a_map_change` (data-gated): a real user log as a
  ghost holds Player instance 1 within 30 m and sounds. It is released at 400 m, and after a map
  change it claims instance 1 and sounds again.
- skate-mods:
  - command validation per kind, and the event options;
  - the Lua wrappers cross the serde boundary;
  - the bundled test mod runs 400 frames with no Lua error or invalid command, stays within 128
    commands per callback, and its F9 re-centre spawns everything again.

  `check_mod` validates the mod.

## Files

- New: `crates/skate-game/src/world_audio.rs`, `game_audio/world_bridge.rs`,
  `game_audio/state_replay.rs`, `crates/skate-game/src/modding/world_audio.rs`,
  `crates/skate-mods/src/world_audio.rs`, `crates/skate-game/src/tests/audio_state_capture.rs`,
  `mods/world-audio-test/{mod.json,main.lua}`.
- Changed:
  - `game_audio/{world_sources,npc_skaters,native,skate_events,e2e,mod}.rs`;
  - `skate_audio::{player::state (LiteSkater, AudioState::rolling), world::skaters (Slots::with_records)}`;
  - `skate-mods` `vm.rs`, `api.lua`, `lib.rs`;
  - `modding/mod.rs`;
  - `sdk/skate.lua`;
  - `.gitignore` (`mods/world-audio-test/logs/`).

## P3–P5: speech, the NPC instance, traffic, per-model data, retail's order (2026-10-03, headless)

Everything below was built and checked headless against the existing recomp recordings. Neither the game
nor the recomp was launched, and no new recording was made. "The recomp" numbers come from the recordings,
not from a console.

### Ped speech plays (`skate_audio::world::speech_player`, `game_audio/world_speech.rs`)

**How it works.** A ped's speech value change becomes a PedestrianSpeech request (ported before). The speech
manager maps it to an event and gates it (also ported before). The library then picks the record and its takes.
New in this pass:

- **The line plays on one of the living world's two streams.** The interrupt `sub_824A73F0` indexes the
  channel's stream records as `channel × 2 + k`, and the recomp's living-world lines play on two stream
  players.
- **When both streams are busy, the event's tuning decides.** With `+13` it stops a playing line of lower
  priority; with `+14` it does the same when the channel is full. Otherwise the request waits in the
  16-request queue until the event's queue timeout runs out.
- **The stream follows its speaker every console frame.** This was read from the code, not fitted: the
  PedestrianSpeech update `sub_824D9370` copies its outputs into the block that the line's stream reads.
  - Main level: out2. A clip whose name ends in `_f` uses out3 instead (`sub_824A89E8` compares the name with
    `"_f"`).
  - Reverb send: out15.
  - Azimuth: raw out0. Pitch: out1.
  - High pass: out13. Low pass: out14.
  - Security guards, the guard radio and the conversation states use other outputs (spec note
    `world-speech.md`).
- **The cut.** A speaker whose level stays at or below 200 for more than 60 frames has its line stopped
  (vault `6995C510258C9AF6` / `3D8CD05C962FF399`). A speaker that loses its MixMap instance stops at once.
- **The NPC bail grunt.** It goes through the same manager and streams. Its level comes from the skater's
  PlayerSpeech instance (`sub_824DA300`: out2 / out3, send out10, filters out8 / out9).
- **The manager's clock is console time since the start.** Retail's per-speaker timers start at 0, so nothing
  speaks in the first 30 s: the warn's not-follow list holds it, and the shout has its own 30 s.

**`SpeechValue::WARN` is now 53**, the code's warn, which the manager maps to `501_warn`. The state graph's
`DoWarning` (11) entry is commented out in retail. 11 is also `LostInterestEndChase` → `605_chase_terminate`.

**Data and failure modes.**
- The index and rules are part of the normal setup.
- The takes need the speech decode (on by default since 2026-10-25, USER-ASKS 338; `SKATE_SETUP_SPEECH=0` skips it, about 2.4 GB). The dev install now has all 40
  free-roam events: 13,322 takes, 2.3 GB.
- A take is read from disk when its line starts, as retail streams it. The last 24 stay loaded.
- Without the index, speech is off and the log says so once.
- Without the decode, lines are still chosen and logged but stay silent, and the log says so once.

**Checked against the recomp.**
- *End to end* (`a_ped_warns_through_a_stream_at_its_owner_levels`, data-gated): a business man (voice 59)
  warns 3.6 m from the camera and gets `501_59_busm1_Warn_n`; its stream plays at out2 / 32767. At 30 m the
  next warn is `Warn_f` at out3.
- *Levels* (`speech_levels_follow_the_recomp`, from `recomp-research/tools/speech_levels.py` on 163809 /
  164620 / 180430): 39 lines with a joined speaker. Each one was rebuilt at its recorded geometry (ped, camera,
  player).
  - The recomp's stream filters sit at exactly our out14 / out13: 24956 / 77 Hz near a speaker, 3489 / 379 Hz
    far away.
  - First LPF / HPF within 10 % in 4 of the 5 lines that log both.
  - First GAIN: recomp / ours median 1.005 (p10 0.55, p90 1.43, n 34).
  - `_f` lines at 30 / 40 m: 0.092 / 0.044 against our 0.085 / 0.056. out2 alone would be 0.030 / 0.001.
  - SEND within 0.01 in only 10 of 37 lines (open).
- *Overlap*: the recomp shows two persistent stream players (163809: 40 + 6 lines; 164620: 95 + 48; 180430:
  28 + 17).

### The NPC skater's instance: Wheels, Clothing, the bail grunt

Recomp gap run G3 showed which components run for the NPC instance. Now ported:

- **Wheels:** the spin-down streams on layers 0 / 1. Layer 2 is local only.
- **Clothing:** push / plant foley, body slide, cloth falls, with its non-local start block.
- **The bail grunt:** the body poster's first message of a bail (`+422`, once per bail) → event 8206 for the
  skater's voice (`NpcSkaterAudio::voice`, the AI skaters' 89–96).

Tricks, Treatment and the OffBoard steps stay off for NPCs, as in retail. Board slide was reached by neither
instance in the runs and is not run.

Data test `the_npc_instance_runs_wheels_clothing_and_the_bail_grunt`: a 476 s user log replayed as an NPC.
- Wheel streams: 0.21 starts/s (recomp NPC 0.24 per held second, other motion).
- Clothing: 0.39 Splice starts/s (recomp 0.27).
- 8 grunts for 8 bails.
- The local player's components posted nothing.

### Pedestrians: per-model data
- **Setup export `world_tuning.ped_models`** (`world_audio.ped_models`, from `aud_characteristics`, keyed by the
  voice): variant, kind, gender, shoe class, far threshold, and a per-voice float. 82 models.
- **The bridge fills these from `PedAudio::voice`:** the shoe class (which sets the footstep samples), the
  security kind (footstep and speech levels), the speech words and the far threshold.
- **Checked** (`recomp-research/tools/ped_models_check.py`) against every PEDAUD line of the gap runs: 20 models,
  3146 of 3146 lines agree on all five fields.
- **Footsteps** still play only for list index < 3 (already in the bridge).

### Traffic
- **Model → record:** setup export `world_tuning.traffic_models` (27 entities). `TrafficAudio::engine` accepts
  `taxi01`, `sedan02` and the like.
- **`TrafficCarPhysics.in0`:** the record writer's relative speed, min(|heading × speed − v_listener|, 35),
  slewed by 100 /s, × 32767 / 35. It opens A11.
- **3DObjPos blocks 1 / 2 / 3** follow the body and the front / rear points at ±1 m along the heading. The
  binding is inferred: block 2 feeds the engine layer, block 3 the exhaust.
- **Checked** (`traffic_rpm_and_relative_speed_follow_the_recomp`, gapg1 VEHAUD):
  - the RPM model lands within 60 RPM in 97.3 % of 451 live sample pairs (p50 0.7 RPM);
  - `+176` lands within 0.5 m/s in 99.2 % while the listener stands. While it moves, 29 of 59 land within 2 m/s;
    the listener's own velocity is not logged.
  - 696 pairs were frozen "stale" records of cars that had left the 40 m list. They were left out.

### Retail's order: process before the tick, update after
- **The world and NPC hosts now run inside the pass:** `native::mixmap_frame` (inputs, the local player's
  process) → `world_sources::frame` (pre) → `npc_skaters::frame_pre` → `native::mixmap_tick` (ticks, the
  local update) → … → the hosts' update → `world_speech::frame`.
- **Before, both ran after the ticks**, so the inputs a tick saw were one console frame old.
- **The local player's sequence is unchanged** (inputs → process → ticks → update), and the hosts touch nothing
  without owners.
- **Proof (`process_before_the_tick_shifts_the_outputs_by_one_evaluation`):** a car driving past the camera through the real MixMap, rendered in the old order (tick, update,
  process) and the new (process, tick, update), with the same start-up history (one empty first tick). The
  TrafficEngine / TrafficSkids / TrafficHorn outputs (level, raw, pitch and filter of 11 outputs each) satisfy
  new[k] == old[k + 1] on 89 of 89 evaluations. So the move is a pure one-evaluation shift: the tick now sees
  this frame's position.
- **The NPC bed now always steps after the local bed's step.** It draws from the local bed's generator, and
  before this the order of those two systems was not fixed.

### Instance managers (checked, unchanged)
The hosts match the measured rules:
- Traffic: the 4 nearest within 40 m, horizontal.
- Peds: the 15 nearest within 50 m (3-D, nearest-first), footsteps for the 3 nearest.
- NPC skater: the first in list order within 30 m, held until 30 m.

### PedBodyFall and Tazer
Gaps at the end of P3; closed below ("Gaps closed").

### Mod surface
**Done in this pass (cheap):**
- `voice` for skaters (the bail grunt);
- traffic model names in `engine`;
- the model-driven ped defaults;
- `info().speech_lines`;
- the mod limits raised to 48 per mod / 128 in all;
- `"enabled_by_default"` in `mod.json` and `SKATE3_MODS_ENABLE`.

**Left for the final moddability pass:** spec §8.2–§8.4 (the content overlay for banks, samples, programs and
speech lines; mod emitters on the native voice graph; tuning read / write; `sdk.audio.post`; audio event hooks)
and §8.6.

### Proofs and tests (this pass)
- **Local player byte-identical:** the e2e bench (13 scenarios + 4 whole sessions, `row` and `fps300`) from a worktree of
  `a4ec831` (`p3_base`, equal to the earlier `sw_new`) against this tree (`p3_new`): all four sets IDENTICAL (26 +
  26 + 8 + 8 outputs). The e2e harness drives the local player's pass itself, so the native split was proved by the
  shift test and by its unchanged local sequence.
- **New data tests** (all pass, `--ignored`):
  - the speech end-to-end;
  - the speech levels against the recomp;
  - the NPC components;
  - traffic RPM / `+176` against the recomp;
  - the one-evaluation shift.
- **New unit tests:**
  - `speech_player` (level ids, far clip, streams, interrupt, queue timeout, cut);
  - the traffic relative speed and points;
  - the opt-in mod;
  - the test mod's objects within the limit.

### Files (this pass)
- **New:**
  - `crates/skate-audio/src/world/speech_player.rs`;
  - `crates/skate-game/src/game_audio/world_speech.rs`;
  - the local tools `speech_levels.py` and `ped_models_check.py` (not published).
- **Changed:**
  - `skate-audio`: `mixer.rs` (`set_direct_dsp`, `set_bank_sample`), `player/contacts.rs` (the grunt flag),
    `world/{keys,mod,peds,skaters,speech_manager,traffic}.rs`;
  - `skate-game`: `game_audio/{library,mod,native,npc_skaters,player_audio,world_bridge,world_sources}.rs`,
    `modding/world_audio.rs`, `world_audio.rs`;
  - `skate-mods`: `schema.rs`, `lib.rs`, `world_audio.rs`, `vm.rs`, `api.lua`;
  - `sdk/skate.lua`, `sdk/AGENTS.md`, `mods/world-audio-test/*`, `tools/asset_pipeline/world_audio.py`.

### Open (after P3)
- **Speech:**
  - the stream's PEAK filter;
  - the reverb SEND for most lines;
  - the `Obj:Speech` inputs a playing line sets (F45 / F19 / F42), probably the ~+100 mB the recomp's near
    lines carry over ours;
  - which of the two streams a request targets;
  - the queue timeout's unit;
  - values 49 (a Splice ring, `sub_824D9C70`) and 29 (the photographer's timer);
  - the main-cast path for pros.
- **Tazer** (`SFXObj_Tazer`) and **PedBodyFall**: the objects are not decoded.
- **Traffic:** the 3DObjPos binding's writer; the horn kinds per model (the AI's); the frozen record of a held
  car (not ported).
- **NPC:** board slide; the walking / jump voices of an NPC on foot; the NPC grain bed against 180430's NPC GREC
  rows (not compared in this pass).

## Gaps closed: Tazer, PedBodyFall, speech details, the NPC board slide and bed (2026-10-03)

Built from the P3–P5 state and checked headless against the existing recordings, plus one scripted background
recomp run for the NPC board slide (user-approved gap runs; muted, one game at a time). All numbers are "the
recomp". Reference only: the TU3 (title update 3) recompilation (skate3recomp / rexglue / Xenia); our own code.

### Tazer (`skate_audio::world::peds::PedTazer`)
- **Decoded:** `SFXObj_Tazer` (Pedestrian object 3, factory `sub_824F12D0`). Its process posts the 9-word
  `c_tazer` packet while the ped audio state's byte `S+80` is set (the manager's bit 17 of the ped entry) and
  releases it when it clears; its update writes raw 0, pitch 1, levels 3 / 2. The program (op 38, ported earlier)
  owns a `c_tazer_grn_play` child post and plays the burst while the packet is held.
- **`S+80` = tazing:** each post came 22 ms after the state graph's `TazeEntity` entry (`TazerCycTime` 2.0 s).
- **Checked** (`tazer_bursts_follow_the_recomp`, session 164620: three zaps, 49 Tazer starts): per 2 s hold ours
  start 19 sounds (recomp 10 / 20 / 19; its first burst stopped after 1.33 s, cause not traced); gaps
  192 / 192 / 128 / 96 ms against 190 / 190 / 130 / 90–100 (the 11th gap 128 against 129 / 131); the order 8, 7,
  then shuffles of 0–6 in both; the first start's gain recomp / ours 0.85 / 0.93 / 1.00 at each zap's geometry.

### PedBodyFall (`PedBodyFall`)
- **Decoded:** `SFXObj_PedBodyFall` (object 2). Its trigger `S+76` is the ped animation's `BodyFallType` channel.
  Each change to a non-zero value starts one Skate_Collisions sound through the collision Splice object: type 8 →
  1184, 9 → 948, any other → 1183 (vault record `923CCB46EF5BF5BA` / `DFEFC9212E0CBD2C`); 8 and 9 round-robin two
  slots. Each type follows its own volume / pitch outputs (8: out1 / out2, 9: out3 / out4, other: out5 / out6).
- **The recordings had it after all:** the starts are Splice starts, not POSTs (75 in six sessions). Retail's
  knock-downs key 9, other, 9, 9 (0.10 / 0.48 / 0.16 s apart) or 9, 8, 8, other, 9.
- **Checked** (`body_falls_follow_the_recomp`, 73 starts): container by type 73 / 73; voice gain recomp / ours
  p50 0.79 (p10 0.25, p90 1.25, n 40; the knocked-down ped is not logged, the ped nearest the player is taken).

### Speech details
- **PEAK filter:** a head shadow by azimuth. The owner's raw azimuth, folded front / back, goes through three
  curves of the speech record (centre 600 → 4000 Hz at the side → 600, gain 0.4 → 0.1, Q 3; setup export
  `world_tuning.speech_voice`). Every recomp PEAK (6 / 6) lies on the curves at one azimuth; ours at the estimated
  geometry 3 / 6 within 10 %. The mixer plays speech on the stream graph (PEAK, Send A, gain, then the filters).
- **Two sends, and the echo submix (ported):** the stream system (`sub_82C5CEF0`) multiplies the gain and both
  sends by the speaker's per-voice float (`S+152`, `aud_characteristics` `2087A3290483BB4F`, 0.8–1.4). The
  **pre-gain** send (`desc+16`, recomp module `+0x570`) carries PedestrianSpeech **out21** (PlayerSpeech out13)
  and feeds the stream slot's **echo submix** (`skate_audio::bus::speech_echo`, built per stream slot by
  `sub_82C5E2D8`: high pass = owner filter 22 (skater 14), delay = camera distance × the record's factor / 344 m/s
  up to 0.15 s (recomputed every 4 console frames, posted only on change), low pass = filter 23 (skater 15), then
  the environment bus). The **post-filter** send (`desc+32`, `+0x7D0`) carries out15 (skater out10) straight to
  the environment bus. Correction to the first decode: it had out15 feeding the echo; the code says the pre-gain
  send does. Recomp: the pre-gain send = out21 in 17 of 29 lines within 25 % (out15: 10); the post-filter send =
  out15 in 19 of 27. Modelled mono: the graph's two-channel Pn21 / Sen0 routing is a unity tap (its gains are not
  traced).
- **Value 49 = a phone call:** the ped's phone rings (CellPhone_Rings container 5, now part of setup) and when the
  ring ends the ped asks for value 64 (`4402_cell_greet`). Recomp 164620: both rings answered 1.00 / 1.19 s
  later; the ring records last 0.84–1.13 s. Without the bank (an install from before this setup) the answer
  follows at once (logged).
- **Value 29 (the photographer):** repeats each second while the game flag is set (`LivingWorldAudio::photo_flag`).
- **`Obj:Speech` inputs:** in0 / in1 / in4 are raised while a playing line's speaker is 37–38 / 75–77 (the guards)
  / 1–29 (the pros); in2 / in3 are not traced. They don't touch regular peds, so the recomp near lines' extra
  ~0.6 dB stays open.
- **Which stream:** the first free one (k0 when both are free: 95 of 109 new lines).
- **Queue timeout unit (settled from the code):** the library's clock `[[0x830CFD94]+16]` is the function behind the
  `GetVisualGameTick` Lua binding (`0x8283C2D8`): visual game ticks, one per rendered frame. The console renders
  at its ~30 fps cadence, so the port's console-frame count is the same unit.
- **Main-cast channel (ported):** the pros' and the special cast's speech (`maincastspeech.big`, speech-manager
  bank 0, channel 0, its own tuning `speech_tuning["0"]`, timers, two streams and mixer bank). Who speaks there:
  a model with no living-world type bit and a cast bit / word (`aud_characteristics` `6F2933E977CF40DD` /
  `14FD437D190677C8`, plus `D6EA428C2B43E23A` for the pro-on-pro lines; setup `world_tuning.ped_models`). Request
  words per event: `sub_824AC898` (`speech_manager::main_cast::request_words`); the main cast's near / far flag is
  1 near, 2 far. Senders:
  - a pro **ped** (`sub_824AC438`): value 29 → `1014_bored` (141), 28 → `601_ai_greet` (11, outside a challenge),
    30 / 51 → 115 or 6, 53 / 54 → 77;
  - an NPC **skater's** own process (`SFXObj_PlayerSpeech` non-local, `sub_824DA1B0`;
    `skate_audio::world::skater_speech`): pros (`sub_824DA768`) say `101_pos` / `150_pro_pos` on a seen trick and
    `104_slam` / `903_race_slam_pc` / `151_pro_slam` on a seen slam, every frame the byte holds; AI skaters
    (`sub_824DAAC0`) `101_pos`, `404` / `405` / `903` / `104_spec_slam`, `105_spec_chase`, on change; its crash
    (`sub_824DB688`, latched) the message pair `131_collide_object` / `906_aislm`;
  - the skater messages by speaker kind (`sub_824DAC00`): bail grunt 8206 / 115, crash 8229 / 125, impact
    8233 / 6, hit reaction 8309 / 253, race punch 8310 / 250, gesture 8291 / 247 (the senders of the last four are
    engine systems the game does not have yet).
- **Main-cast decode:** setup indexes it always (`speech/maincast.json`, 1283 clips) and decodes its free-roam
  events with the living world's (`SKATE_SETUP_SPEECH=1`): **6596 takes, 3.0 h, 922 MB** of 44.1 kHz PCM16.
- **Checked:** `main_cast_lines_are_reachable`: all **82** main-cast lines the recomp streamed in 11 sessions
  (906 `AiSlam` ×45, 101 `pos`, 150 `pro_pos`, 104 `Slam`, 130 `col`, 201 `grunt`) are reachable through our
  words for their speaker (76 of them of an event the port sends; 130 `_col` has no ported sender).
  `a_pro_skater_says_a_main_cast_line`: Ryan Smith's model as an NPC skater sees the player's trick and the main
  cast streams `101_24_Smit_pos` at its PlayerSpeech level.
- **Re-verified** (`speech_levels_follow_the_recomp`, 39 lines, now with the voice float): filters 4 of 5, gain
  recomp / ours median 0.988 (p10 0.55, p90 1.36), pre-gain send = out21 17 / 29, post-filter send = out15 19 / 27,
  PEAK on the curves 6 / 6.

### NPC skater: the board slide and the bed
- **Board slide ported for the NPC instance:** retail's slide code has no local test and its input (the
  loose-board state) is computed per skater, so `NpcSkater` runs it from `NpcSkaterAudio::loose_board` (ghosts:
  per row of their log). A scripted background run (Mega-Park, 4 min standing, the PLAYERPOST hook, 0 malformed
  lines): instance 1 was held 78 s in 12 holds, no NPC bailed, no slide post by either instance: still unobserved.
- **NPC grain bed vs session 180430's NPC rows** (the GREC rows of the NPC object, filtered by owner, joined with
  the board's distance; `npc_bed_follows_the_recomp_rows`, 289 straight-roll rows): truck A gain recomp / ours
  0–10 m 0.097 / 0.108, 10–20 m 0.045 / 0.065, 20–30 m 0.005 / 0.014; pitch 0.93–0.97 / 0.96. The far bands are
  louder in ours: the lookups measure the distance to the local skater, which the rows don't give. (Corrected
  2026-10-04: the lookups read the camera's distance and azimuth; the test's geometry was the cause. See
  "Follow-ups".)

### Mod surface and data (moddability)
- Engine: `PedAudio::{tazing, body_fall}`, `PedTazerEvent`, `PedBodyFallEvent`, `NpcSkaterAudio::{loose_board,
  reactions}`, `NpcSkaterReactionEvent`, `LivingWorldAudio::photo_flag`; ped / skater `voice` 1–96 (the pros and
  the special cast speak on the main cast).
- Mods: ped options `tazing`, `photo_flag`; lite-skater option `loose_board`; events `tazer` (seconds) and
  `body_fall` (kind), skater event `reaction` (value `slam` / `slam_b` / `trick` / `crash` / `chase`, `by`); speech
  value 49 works through `speech`; `voice` accepts 1–96. The test mod: F7 tazer, F6 a retail knock-down's keys, F5
  a phone call, F4 the photographer, F3 the ghost sees your trick, F2 the ghost switches between its AI voice and
  a pro's (24) and crashes.
- Setup data (`world_tuning`): `ped_objects` (fall containers, eq bus, ring container, the tazer hold from the ped
  state graph) and `speech_voice` (the PEAK curves, the echo delay factor / per-metre / cap / refresh frames);
  `ped_models` gained `cast_bit` / `cast_word` / `cast_word2`; `speech_tuning["0"]` (the main cast's) is now used;
  `speech/maincast.json` + the opt-in decode; `CellPhone_Rings.bnk` decoded with its patch tree;
  `world_audio.py` joined the audio group's fingerprint. Every field defaults to the retail value and the world
  tuning overrides it.

### Proofs and tests
- Local player byte-identical: the e2e bench (13 scenarios + 4 sessions, `row` and `fps300`) from this pass's
  starting point (`wg_base`) against the result (`wg_new`, and again after the echo submix and the main cast:
  `wg_new2`): all four sets IDENTICAL (26 + 26 + 8 + 8 outputs) both times.
- Main-cast / echo tests: `main_cast_lines_are_reachable` (82 / 82), `a_pro_skater_says_a_main_cast_line`; units
  `skater_speech` (pro / living-world choices, crash latch), the echo delay and the echo graph; the mod event
  `reaction` and the test mod's F3 / F2.
- New data tests: `tazer_bursts_follow_the_recomp`, `body_falls_follow_the_recomp`,
  `npc_bed_follows_the_recomp_rows`, `the_phone_ring_lasts_until_the_recomp_answers`; re-verified
  `speech_levels_follow_the_recomp`. New unit tests: peds (tazer, falls, ring, photographer), the PEAK curves, the
  mod options / events, the test mod's new keys, the setup export.

### Still open
(Followed up 2026-10-04: see "Follow-ups" below for in2 / in3, the near lines, the slot 30 / 31 repeat times, values
30 / 51, the crash line (the announcer's), the NPC bed's distance input and the NPC slide.)
- Speech: Obj:Speech in2 / in3; the near lines' ~0.6 dB; the echo graph's two-channel routing gains; the
  main-cast speaker-slot repeat times for slots 30 / 31 (record `+52` / `+56`); values 30 / 51 also stopping the
  speaker's playing line; the game modes the skater choices test (19 / 20 / 55: free skate is none of them, the
  port passes 0); the cameraman line the crash can request; senders of the hit-reaction / race-punch / gesture /
  skater-collision lines (no such engine systems yet).
- NPC: the board slide is ported but unobserved; the walking / jump voices.
- Traffic: the 3DObjPos binding's writer; horn kinds per model.

## Follow-ups: speech inputs, the main cast's repeat times and stops, the NPC bed's distance, the NPC slide (2026-10-04, headless)

Branch `audio/world-followups` (former #44, now merged into `gameplay/audio`, #32). Worked from the code and the existing
recordings only: no new sessions, no scripted run. All numbers are "the recomp". Reference only: the TU3
recompilation (skate3recomp / rexglue / Xenia); addresses and constants are facts, the code is our own.

### Speech: `Obj:Speech` inputs 2 / 3, and the near lines' ~0.6 dB
- **Inputs 0 / 1 / 4 look at the main cast's streams only (fix).** SFXObj_Speech's process (`sub_824E2050`)
  walks the stream system's first two stream records (`[mgr+8]+1120` / `+1124`). Those are streams 0 and 1:
  channel 0, the main cast (the pro path stops "its" stream by the same index, `sub_82C5E1D8(sys, k)`). So
  in0 / in1 / in4 are raised by a main-cast line whose clip voice is 37–38 / 75–77 / 1–29. The main cast has voice
  77 (18 clips) and 37 / 38; the living world's guards (75 / 76) speak on channel 1 and raise nothing. The port
  used to raise in1 for a living-world guard line. In free skate that was inaudible (in1's duck F42 → Global C3 →
  the Music outputs is scaled by `Master.in7`, which free skate leaves at 0; F19 feeds nothing), but the input now
  follows retail: only the main-cast player's speakers count.
- **Input 2** = the speech system's word `+0x1BD28` == 2. The only code that drives that word is the scripted
  dialogue path (`sub_824A4EE8`, request ids ≥ 5000, categories 31–40: the story / challenge lines; set to 1 when
  its stream starts, cleared by `sub_824A54D0`). **Input 3** = a main-cast stream whose block has `+60` == 0 and
  `+93` set (`+60` is written by the stream start `sub_82C5DC80` from its fourth argument). Neither happens in
  free roam; both stay 0. What they drive: in2 → F29 (−200 mB on the music) and F221 / F222 (with Pause.in2); in3
  → F46 (−200 mB on the music), F33 / F34 (+0 mB). None of them touches ped speech.
- **The near lines' ~0.6 dB is gone.** With the per-voice float (ported in the gaps pass), the 34 recomp lines
  with a gain give recomp / ours median 0.988 (−13 mB); near lines (camera < 10 m, 24) −22 mB, far lines (10)
  +45 mB: no near offset is left. Checked against the one global term in PedestrianSpeech's level that the port
  leaves at 0, `A[Global.112]` = `VU.in0` (+200 mB at full): the VU object (`SFXObj_VU`, process `sub_824EDBE8`)
  meters a mixer bus and slews the result into in0 / in1. If it explained an offset, the gain ratio would grow
  with the game's loudness; against the capture's RMS over the 500 ms before each line (`audio.f32`, 34 lines)
  the correlation is −0.20 (RMS terciles −4 / −86 / −150 mB). Not ported (it would also move the ambience,
  emitters, cameraman and ped SFX); noted under Still open.

### Main cast: the speaker-slot repeat times, values 30 / 51, value 29, the "cameraman" line
- **Repeat times of speaker slots 30 / 31 (ported).** `sub_824AC560` takes the event's repeat time from the
  speech record's `+24`, except for speaker slot 31 (`+52`) and 30 (`+56`), even when that value is 0. The
  speaker slot is the speaker's model (`S+84`): 31 = `skate_coach`, 30 = the special-cast model
  `47231014932CE8B3`. Examples from the vault: event 0 repeats after 20 s, for slot 31 after 30 s, for slot 30
  after 10 s; event 3: 25 / 0 / 25 s; event 141 (`1014_bored`): 30 / 0 / 20 s. The living world's request
  (`sub_824ABA18`) has no such override.
  - Setup: `world_tuning.speech_tuning[bank][event]` gains `repeat_speaker_31` / `repeat_speaker_30`.
  - Engine: `EventTuning::speaker_repeat` (speaker, seconds), used by `request_main_cast` only. A mod's tuning
    overlay can list more pairs in `speaker_repeat`.
  - An install from before this change has neither field: the main cast then keeps `+24` for every speaker, as
    before. A setup rerun brings them.
- **Values 30 / 51 stop the speaker's line first (ported).** `sub_824AC438`: with the speaker's speaking byte
  (block `+76`) set and its stream (block `+72`) neither −1 nor 2, `sub_82C5E1D8` stops that stream before the
  request, whether or not the new request then passes the gate. PedestrianSpeech remaps the bump values 7 / 8 to
  30 (after a 29) or 51, so a knocked pro cuts its own line short and says `202_impact_react` or
  `201_Light_Impact_Grunt`. Ported as `SpeechPlayer::stop_speaker` (the stream frees at once, the queue stays),
  called by the host before the request; `main_cast::stops_line`.
- **Value 29 asks twice (correction).** The pro path requests `1014_bored` (141) and then `1002_amb_chat`
  (136): the code calls `sub_824AC560` with 141 and falls through to the common call with 136. The first port
  read 136 as a word. `main_cast::events_for_value` returns both, in order.
- **The crash line is the announcer's, not the cameraman's (not ported).** `sub_824DB688`, after the crash
  message, compares the skater's camera distance (its record `+96`, written by `sub_824B6E80`) with the speech
  record's field `887C1D3324B12C4A` (12.0 m). Closer, it reads the model's `aud_characteristics` field
  `14B23B4527AF919E` (`SPCH3Type_pro_id_ANN`: the pro's bit, e.g. `chris_cole` 0x2) and, when it is non-zero,
  requests event 24708 = bank 3 (0x6000) event 0x84 = the announcer's `480_slam_pro` (fields 1 / 2 / 3, 26
  records) with the bit in word 2, through `sub_824AA858`. That is the announcer channel
  (`announcerspeech.big`, bank 3, 63 events), which is not indexed, decoded or managed yet; its request has its
  own game-state gates (the challenge's announcer `+1036`, `+1089`, the record's `+48` byte). Ported in step 4
  ("The announcer channel" below): free skate never plays it.

### NPC grain bed at distance: the input was the camera's all along
- **The code.** The Player slot's level lookup B2 (SkateBoard out1 / out2 …) reads its 3DObjPos block's
  input 1 (distance) and input 3 (azimuth) with per-quadrant ranges: 4–50 m ahead, 4–40 m at the sides, 1–30 m
  behind (MixMap catalog). `sub_824AE6E0` writes input 1 = |listener `+0` − emitter| and input 3 = the azimuth in
  the frame (listener `+0` pulled back 0.25 m, listener `+32`); inputs 0 / 2 use listener `+64` / `+96`.
  `sub_8248CC08` fills the listener: `+0` / `+32` from the camera's matrix, `+64` from the skater list's first
  entry (the local skater). So the bed's level reads the **camera** distance and the **camera-frame** azimuth:
  what the port already writes (`ObjPos::write`). The earlier note ("the lookups measure the distance to the local
  skater, which the rows don't give") was wrong for the Player slot; it holds for the ped speech lookups (input 0).
- **What was wrong was the test.** `npc_bed_follows_the_recomp_rows` placed the NPC dead ahead of the camera for
  every row: always the 4–50 m quadrant. The recomp's NPC passes at the side or behind as often.
- **The rows now carry their geometry** (local tool `npc_grec_rows.py`): the NPC board (SKATEB, matched by speed,
  with the local skater's own board (the track that follows the local SkateBoard's GREC speed, median error
  0.11 m/s) left out, and rows where two boards match within 0.3 m/s dropped), interpolated to the row; the
  camera (WPPOS, interpolated); the camera's view (not logged: the direction from the camera to the local board,
  or the camera's own motion when that board is more than 8 m away). The test rolls our NPC bed through each
  row's own geometry (camera and local skater moving with the local board's velocity).
- **Result** (session 180430, 176 straight-roll rows at ≥ 1 m/s; truck 0 A gain recomp / ours, medians):

  | camera distance | rows | recomp / ours | before (NPC dead ahead) |
  |---|---|---|---|
  | 0–10 m | 34 | 0.101 / 0.123 | 0.097 / 0.108 |
  | 10–20 m | 80 | 0.050 / 0.054 | 0.045 / 0.065 |
  | 20–30 m | 58 | 0.0091 / 0.0111 | 0.005 / 0.014 |

  Row by row (recomp gain > 0.002, n 165) recomp / ours p10 0.41, p50 0.94, p90 1.13. Per sector the azimuth
  shows in both: 10–20 m behind 0.021 / 0.028 against 0.055 / 0.065 ahead; 20–30 m behind 0.0020 / 0.0022,
  ahead 0.028 / 0.028. Pitch 0.97 / 1.00. The scatter is the estimated view and the 250 / 500 ms
  interpolation. No engine change.

### NPC board slide: observed
- Session 180430, 152.13 and 152.30 s: two `c_board_slide` posts (class 15 from the slide process, `824CB470`)
  while the local skater rolled on four wheels at 8.5 m/s and instance 1's skater (board `468DC6B0`, about 15 m
  away, contacts `0000001` at 151.98 s) had no wheels down and slowed from 4.8 to 3.7 m/s: the NPC's bail (local
  tool `npc_slide_posts.py`, the two SkateBoard owners' GREC rows of this pre-fix session; the other 13 posts in
  the session are the local skater's own bails). Each post is followed 62–68 ms later by three `board_scrapes`
  voices (18 / 19 / 24, then 4 / 8 / 10) at gains 0.005–0.012, with the NPC's Clothing body slide
  (`Bodyslide` 17 / 15) beside them. So retail's instance 1 does play the board slide, as ported (no local test
  in the slide code). Its loose-board state is not logged, so the post timing is not compared.

### Moddability
- Speech tuning: `repeat_speaker_31` / `repeat_speaker_30` from setup, `speaker_repeat` pairs from a mod's
  tuning overlay (`world_tuning.speech_tuning`).
- A mod ped with a pro voice (`voice` 1–29) and speech values 29 / 30 / 51 (`speech`) reaches the double
  request and the stop; `Obj:Speech` follows whatever the main cast plays.

### Proofs and tests
- Local player byte-identical: e2e bench (13 scenarios + 8 sessions, `row` and `fps300`) from this branch's
  starting point (`f0`) against the result (`f1`): all four sets IDENTICAL (26 + 26 + 16 + 16 outputs).
- New: `main_cast_values_request_their_events_in_order`, `main_cast_speaker_slots_30_and_31_have_their_own_repeat_time`
  (skate-audio), `a_speaker_s_new_value_stops_its_playing_line` (speech player),
  `a_pro_ped_s_impact_value_stops_its_line_first` (data-gated: value 30 stops the playing `202_24_Smit_Rct`
  line, the next one follows; in4 on for all 12 frames, in1 never); the Python export test checks `+52` / `+56`.
- Changed: `npc_bed_follows_the_recomp_rows` (per-row geometry; every band within 0.67–1.5, row median within
  0.8–1.25).
- Suites: `cargo build --locked` ok; skate-audio 229 + integration pass, all its ignored data tests pass; game_audio 44 pass, its 34 ignored data tests pass (private-data env set); Python `test_world_audio` 7 pass; skate-mods passes except `skyline_every_component_is_real_and_drives_through_ground_contact` ("Skyline GLB missing": the gitignored model is not in a fresh worktree; unrelated).

### Files
- `crates/skate-audio/src/world/speech_manager.rs` (`EventTuning::speaker_repeat`, `request_main_cast`,
  `main_cast::{events_for_value, stops_line}`), `speech_player.rs` (`stop_speaker`, `stopped`),
  `crates/skate-audio/tests/world_speech.rs`.
- `crates/skate-game/src/game_audio/world_speech.rs` (the stop, the double request, `Obj:Speech` from the main
  cast's streams, the new test), `world_sources.rs` (the tuning fields), `npc_skaters/tests.rs`.
- `tools/asset_pipeline/world_audio.py`, `test_world_audio.py`.

### Still open
- ~~The announcer channel (and with it the crash's `480_slam_pro`).~~ Ported, see "The announcer channel" below.
- `VU.in0` / `in1` (the VU meter of a mixer bus, `sub_824EDBE8`): which bus, the meter's scale; not needed for
  the speech levels by the evidence above.
- The main-cast request gate: the record's `+48` byte lets a line through while `sub_8279E180` holds (a game
  state not traced; free roam passes).
- The NPC slide's timing against the NPC's loose-board state (not logged); the NPC's walking / jump voices.

### The announcer channel (2026-10-04, follow-up step 4)
Retail's contest commentator: `announcerspeech.big` (405 clips, 2,822 takes, 3.4 h, 36 kHz mono; 63 events of
speech bank 3), speech channel 3 (stream records 6 / 7). Read from the TU3 recompilation (reference only) and
checked against all 38 recorded sessions; no new session was run.

**Mechanism.**
- **Two announcers.** Models 35 and 36 (`aud_characteristics` parent `ip`, `SPCH3Type_char_ID_Ann` 1 / 2). Every
  announcer record's first field is 1 or 2, and its clips carry that voice (`480_35_…` / `480_36_…`).
- **Who is announcing** is the running challenge's choice: the system word `+1036`, written from the challenge
  record when the challenge changes (`sub_82488330`), and 104 ("nobody") when no challenge runs.
- **The request** (`sub_824AA858`):
  - the event's vault tuning (bank 3, already exported in `speech_tuning["3"]`);
  - the game-state gate the main cast also has (record `+48`, `sub_8279E180`, system `+104` / `+997`; not
    modelled, as for the main cast);
  - word 0 = the announcer's id (from model `+1036`'s `char_ID_Ann`) when the caller left it 0 and a challenge
    names one; word 8 = 5 / 6 (system `+1089`);
  - the manager's timers are kept per announcer (104 → slot 35);
  - the gate (`sub_824A8C78` + `sub_824A75F0`: one `rand()` for the probability; the timers `+40` / `+44` compare
    challenge clocks);
  - the interrupt on channel 3, then the library request with each event's words (`sub_824AAC40`, two jump
    tables; e.g. `480_slam_pro` = words 0 / 2 / 11, `426_results` = 0 / 2 / 11 / 32 / 7, most events word 0 only).
- **Free skate never plays an announcer line.** Without a challenge, word 0 stays 0 and no record matches.
  SFXObj_Announcer's own commentary (`sub_824CF350`, 11 requests) also returns at once unless `+1036` is 35 / 36.
- **The senders.** There are 39 calls in all:
  - the challenge commentary (tricks, grinds, bails, big air, leaders, results, timers);
  - the contest results (`sub_8248C988`);
  - the scripts' `PlayAnnouncerSpeech` binding;
  - one that runs in free skate: an NPC skater's crash (`sub_824DB688`). Below 12 m from the camera it asks for
    `480_slam_pro` with word 2 = the model's `SPCH3Type_pro_id_ANN` (pros 1–29 only).

  So in free skate a pro's crash near the camera is gated and draws one `rand()`, then finds no line.
- **Recordings agree.** In 38 sessions, `announcerspeech.big` was read only at boot (its 5 index reads), never
  for a clip. Those sessions include 37 NPC crash lines (`906_aislm`), several of them with a skater well inside
  12 m of the camera.
- **The voice.** The stream block at `mgr+0x1B898` is refreshed every frame from SFXObj_Announcer (registered at
  `mgr+0x1B8F8`; its `sub_824D07B8` copies the Global Announcer object's outputs):
  - level = out2 × a vault multiplier, truncated to an integer. The multiplier (`sub_824A8250`) depends on the
    console language (English, Italian and Spanish use the `other` arrays: 1.1 / 1.25 / 1.1 for speaker 35 / 36 /
    31; with the challenge byte `+1044`: 0.9 / 1.0 / 1.0);
  - pitch out1, azimuth out0 (no MixMap term: centre), high pass out4, low pass out3, environment send out5;
  - voice float 1.0, PEAK flat (96 kHz, gain 1, Q 3), no echo send (`sub_824A3C28`'s constants);
  - no level cut (that is the ped / skater owners' rule).

  While a line plays, SFXObj_Announcer raises `Announcer.in0` (`sub_824CF218` / `sub_824A61C0`). The Global ducks
  the mix with it (F2 / F37 / F47 / F50 / F95 / F172 / F241) and the reverb (F208 / F209 / F212).

**Ported.**
- `skate_audio::world::announcer`: the words (`request_words`), `Context` (character, its word, word 8, slot),
  `SpeechManager::request_announcer`, `AnnouncerLevel`, `crash_block`.
- `speech_player::announcer_outputs`, plus `SpeechPlayer::no_cut` for the announcer's streams.
- `skater_speech`: `Say::Announcer(480_slam_pro)` after the crash message.
- The host (`game_audio/world_speech.rs`): the announcer channel (bank `ANNOUNCER_BANK`), the crash request
  (distance below the export's `announcer_crash_m` and a non-zero pro id), engine / mod requests, its stream
  values, and `Announcer.in0` (written only when it changes).
- Engine: `LivingWorldAudio::{announcer, mod_announcer}` (None = free skate) and the message
  `AnnouncerSpeechEvent { event: Id | Name, pro, words }` (the `PlayAnnouncerSpeech` equivalent).
- Mods: `sdk.world_audio.announcer(character)` (cleared when the mod stops) and
  `sdk.world_audio.announce(event, {pro, words})`; `info().announcer`.
- Setup: `speech/announcer.json` (always). `world_tuning.speech_voice.announcer_crash_m` and `.announcer_level`
  (all six arrays). `ped_models[..].announcer_id` / `announcer_pro`. The opt-in decode
  (`SKATE_SETUP_SPEECH=1`) covers `ANNOUNCER_EVENTS` = 480 only, the one free-skate sender (571 s, ~41 MB);
  other events are chosen and logged but stay silent until decoded.

**Proofs and tests.**
- Local player byte-identical to `f1` (e2e bench, 13 scenarios + 8 sessions, `row` and `fps300`): all four sets IDENTICAL (26 + 26 + 16 + 16), with the old install and again with the announcer data staged.
- Suites: `cargo build --locked` ok; skate-audio 235 + integration pass and all its ignored data tests (7 speech data tests); game_audio 44 pass, its 35 ignored data tests pass (private-data env set); Python `test_world_audio` 8 pass; skate-mods passes except the unrelated `skyline_every_component_is_real_and_drives_through_ground_contact` (gitignored model missing in the worktree).
- Unit tests: `announcer::tests` (words, block, free skate draws and finds no line, the per-announcer timers, the
  level scale), `the_announcer_takes_its_object_outputs_and_keeps_playing_when_quiet`, the skater speech crash
  order, the mod options / commands, and the Python export.
- Data tests (ignored, private data):
  - `the_announcer_index_holds_two_announcers` (63 events, every record announcer 1 / 2 with its voice, one shipped exception: `118_DbailSpc` plays `117_36_DbailGen` for announcer 1);
  - `slam_pro_lines_need_a_challenge_announcer` (every `480_slam_pro` record refused in free skate, reached
    with its announcer and pro word);
  - `the_recordings_never_stream_an_announcer_line` (38 sessions: index reads only, 0 clip reads);
  - game_audio `a_pro_crash_near_the_camera_asks_the_announcer` (free skate: asked, no voice; announcer 36:
    `480_36_slam_pro_Smit` at out2 × 1.25 (gain 0.442 = expected) with `Announcer.in0` up for its 88 frames; 20 m: not asked; a request by name reaches the gate).

**Still open (announcer).**
- The challenge clocks behind the timers `+40` / `+44` (system `+900` / `+904` / `+908`). The port uses the time
  since the announcer was named, and no limit for the second.
- System `+1089` / `+1044` (the port: 0).
- The record `+48` gate (shared with the main cast).
- The interrupt's fifth argument (`sub_824A61C0(mgr, 1)`).
- The other 38 senders: they belong to challenge modes the engine does not have.
- Levels against a recording: none exists (free roam never streams the announcer). A challenge session in the
  recomp would give one.

## Session marker sounds (2026-10-04, branch `audio/respawn-marker`; former #49, now part of #32)

### Problem
Setting a session marker (LB + D-pad down) and returning to it (LB + D-pad up held) were silent in our engine; the
user reported both missing. Retail also plays a sound when LB opens the marker menu.

### Root cause
Retail plays these from its **front-end sound system**, which the native port did not have: the session marker
asks for `fe` records (the vault class of UI sounds), and a front-end audio object plays each record's
`sk8_menu` Splice sound. Neither the `sk8_menu` bank nor the `fe` records were exported by setup, and nothing in
the engine asked for them.

### Evidence (the recomp, TU3; reference only)
- **Who asks, and when.** PlayerUI `sub_82898FC8` (UpdateSessionMarker, already the source of our marker logic):
  - on the Place Marker action (42): the place sound `cellphone_place_marker` when the marker went down, the
    error sound `cellphone_marker_error` when it could not (the press always sounds one of the two);
  - in the tick the Go To Marker hold completes and the skater is moved: `cellphone_goto_marker`, once per hold
    (not within 0.5 m of the marker, where nothing relocates).
  - The cellphone UI `sub_826682B0` plays `cellphone_activate` when LB opens the menu (state 2, input 4, when its
    mode check `sub_82668BD8` allows the menu).
  - Each asks through `sub_825DFAF0` with the record's key (the vault name hash: `0D6C88A3B91C828F` place,
    `66B3AFE3B602918C` error, `7F135F9FD28F7F21` go-to, `47FE75BF61F19941` activate), which queues a message to the
    audio thread.
- **How it plays.** The front-end object lives at the audio system + 64 (constructor `sub_82495288`, 10 slots of
  32 bytes). `sub_824955B8` reads the record: field `+8` = the `sk8_menu` sound (≥ 1 plays), `+4` = its level
  (≤ 0 plays nothing); the first slot that is neither pending nor playing takes it (`sub_82495828`; none free =
  dropped). Each audio frame `sub_824958F0` starts a pending slot's Splice sound with the block
  `[level × volume, 1, 0, 0, 1, 1]` and then updates it with `[level × volume, 1, 0, dt, 1, 1]` until it ends.
  Output: the mastering graph (`[[system+8]+52]`), so no environment send and no eEQChain bus; 2-D (the Splice
  members' own pan offsets, no listener geometry).
- **The volume word** is `[[X+88]+40]` × 1/32767 (X = the audio system; `+44` for banks other than sk8_menu /
  HOM_Set_1). A watch-list run (scripted, muted, background) read **14568** there, next to 32692 at `+48`, and the
  port's MixMap gives exactly 14568 / 14568 / 32692 / 32692 as the **Master controller's outputs 0–3** with free
  skate's Master inputs. So the volume is Master output 0 (−7.04 dB), not a constant.
- **The records** (setup export, `fe` class, 237 records): activate → sk8_menu 235 at level 0.5, place → 237 at
  1.0, error → 209 at 1.0, go-to → 236 at 1.0 (each a container of one record: 2, 3 or 4 layered members).
- **Measured** (marker posts in existing sessions: the user's `audiox_bail_20261003_094336` and
  `audiox_20261003_095054`, scripted `bailrun_ok*` and `all_20261002_223613`, all 0 malformed; 50 activates, 11
  places, 18 go-tos; plus the scripted run `marker_fevol`): SPLC bank 5 (= `sk8_menu`) 235 when LB is pressed,
  237 at the place, 236 about 200 ms after LB + up (the hold's duration below 100 m). Steady voice gains: activate
  sample 58 0.6288, 59 0.1572; place 58 0.8892, 40 (fades in) peak 0.3048, 59 0.6288; go-to 58 0.6288, 36 0.4446,
  40 peak 0.2223, 59 0.6288. No session holds the error sound (209); a scripted try to provoke it placed the
  marker instead (running on foot is allowed), so its playback rests on the code and the export.

### Change
- **Setup** (`tools/asset_pipeline/audio_export.py`): `sk8_menu.bnk` joins `BANKS` (decoded samples, its patch
  tree for the native Splice player); new `frontend_sounds()` writes the manifest's `frontend` key (every `fe`
  record: sk8_menu id, level, HOM id, moment, alt-bus flag; inherited fields resolved).
- **`skate_audio::frontend`** (new): the front-end object (10 slots, request / frame / clear), the record table
  `FeTable` and the marker record names `frontend::marker::{ACTIVATE, PLACE, ERROR, GOTO}`.
- **Game** (`game_audio/frontend.rs`, new): `FrontendHost` loads the table and the bank at start, takes
  `ui_audio::FrontendSound` messages and runs the object once per audio pass after the MixMap tick (dt = the
  pass's host ticks, like the player's Splice sounds), volume = Master output 0 / 32767 each pass.
- **Engine-facing** (`crates/skate-game/src/ui_audio.rs`, new): `FrontendSound` (play an `fe` record by name),
  `SessionMarkerEvent { action: Opened | Placed | Refused | Returned }` and the data resource
  `SessionMarkerSounds` (action → record, retail's records by default; `None` silences one).
- **Session marker** (`session_marker/mod.rs`): sends the four events where retail asks for its sounds (LB's
  press, the place / refusal, the relocation tick when the teleport request is accepted).
- **Mods:** `sdk.audio.frontend(name)` plays any `fe` record (command `audio_frontend`, name validated, unknown
  names silent; one-shots, nothing to clean up on disable; at most 10 play at once, as retail); every running mod
  gets `on_event {name = "session_marker", action = "opened" | "placed" | "refused" | "returned"}`. Remapping or
  muting the marker sounds from a mod (a content-overlay identity for `fe` records / sk8_menu) belongs to the
  audio modding (doc 16, former #36, now part of #32), which was not in this branch at the time: the identities are the `fe` record names and the
  `SessionMarkerSounds` resource.

### Files
`tools/asset_pipeline/audio_export.py`, `crates/skate-audio/src/{lib.rs, frontend.rs}`,
`crates/skate-game/src/{main.rs, app.rs, ui_audio.rs, session_marker/mod.rs}`,
`crates/skate-game/src/game_audio/{mod.rs, native.rs, library.rs, frontend.rs}`,
`crates/skate-game/src/modding/{mod.rs, audio.rs}`, `crates/skate-mods/src/{vm.rs, audio.rs, api.lua}`,
`sdk/skate.lua`, this doc and `audio-specs/world-audio-hookin-spec.md` §11.

### Verification
- `marker_sounds_follow_the_recomp` (data-gated): each record through the real runtime and install data: the
  samples in the recomp's order, steady gains within 1 % (ours 0.62875 / 0.15719, 0.88919 / 0.29842 / 0.62875,
  0.62875 / 0.44459 / 0.22230 / 0.62875; the fading sample 40 within 5 %), delays within 50 ms (ours 0 / 53,
  0 / 69 / 139, 0 / 171 / 187 / 288 ms against 10 / 46, 13 / 77 / 144, 8 / 162 / 189 / 278).
- `session_marker_events_start_retails_sounds` (data-gated): events → records → runtime starts (235, 237, 236);
  nothing starts on a frame without a pass.
- Unit tests: the object (slots, levels, drops, repeats, clear), the record keys = retail's constants, the event
  → record mapping and its remap / silence, the mod command's validation.
- The headless e2e bench (scenarios + real sessions, row and fps300) is byte-identical to `gameplay/audio`
  (the e2e harness does not load the front-end bank; the frontend runs only on requests).
- Suites: skate-audio (incl. data tests), game_audio / ui_audio / session_marker (incl. data tests), skate-mods
  (only the known `skyline_every_component_is_real_and_drives_through_ground_contact` fails).

### Open questions
- The start block has no record gain (by the code), so the first voice of a sound plays one pass at level /
  record gain before the update; the recomp shows the steady level at once because its frames (hundreds per
  second) land the start and the first update in one render drain. Ours follows the code.
- The error sound is unmeasured (no recording has one).
- Our engine's place conditions (`session_marker` validation) were not compared with retail's here; in the
  scripted run retail placed while the skater ran on foot.
- Retail's mode gate on the cellphone menu (`sub_82668BD8`: game modes, challenge states) has no counterpart in
  free skate; when challenges exist, `SessionMarkerSounds` / the events are where to gate.
- The `alt_bus` flag (photo records) and the HOM / moment fields are exported but not played (no caller yet).

### Follow-up: the static noise on Go To Marker (2026-10-04, investigated, nothing new to port yet)
The user, after listening: the marker sounds sound good, but no static noise plays with the transition back to
a marker as it does in retail.

**What retail does (the recomp, TU3; reference only).** Searched for a sound outside the `fe` record and found none:
- **Code path.** Go To Marker in `sub_82898FC8` does three things: (1) every UI tick of the hold it sends message
  `0xFAF37902` (= `cMsgTeleportEffectAmount`, {+16 = hold progress 0..1}) to the message hub `[0x830CFD94]+64`, and
  for 3 more ticks after the jump with progress 1; (2) in the relocation tick it queues a `Flow::Teleport` request
  (96 bytes, `sub_825582D0`, type 3, session-marker flag) to `[[0x83083BCC]+156]`; (3) it asks for
  `cellphone_goto_marker`. The only listener of `0xFAF37902` in the game code is the **VisualDirector**
  (`VisualDirector::VDSimulationState`, constructor `sub_827A96F8`, handler `sub_827A9C60` stores the amount at
  `+48`, read by the presentation builder `sub_827AAF10`): the screen static is a visual effect only. (The other
  references register the message for scripts.) Our engine already draws it (`session_marker/effect.rs`, noise
  `noise.rs`).
- **No other UI sound.** All 361 calls of the `fe` request `sub_825DFAF0` in the game code name their record:
  `cellphone_goto_marker` is asked once, in the relocation tick; no teleport, fade or flicker record is asked by the
  teleport code (`online_flicker`, `core_fade`, `hom_transition` belong to other screens). Tool:
  a local script (not published).
- **No other sound in the recordings.** Around every go-to of the user's `audiox_bail_20261003_094336` (6) and
  `audiox_20261003_095054` (2) and the scripted `bailrun_ok*` (15; all 0 malformed), every POST / SPLC / PLAY /
  READ in −3 s … +5 s: the only sounds that follow each go-to are sk8_menu 58 (+3 … +13 ms), 36 (+156 … +168),
  40 (+183 … +199) and 59 (+273 … +290), i.e. the record. Others are the ride itself: SenseOfSpeed's rocket
  layer (POST 12 at ~+68 ms, `sub_824E7980`) and, while riding, one pass-by whoosh (Sk82_Whsh_Bys, SPLC bank 3
  id 91 from `sub_824D2C70` at +70 … +79 ms after 7 of the user's 8 go-tos, none in the bail runs; voice gains 0.003 … 0.021, the same as the
  pass-bys elsewhere, and not visible above the bed in the capture). No stream starts, no MixMap-driven duck.
  Tools: local scripts (not published).
- **The recomp's own output** (the capture of 20 go-tos: the user's 8 in `audio.f32`, 12 of the bail runs in
  `trace.f32`; aligned on sample 58's onset, 100 … 150 ms after the SPLC): above 4 kHz only two noise bursts stand out of the bed (−53 … −56 dB at 0 … 40 ms and −49 … −53 dB at 270 … 300 ms
  after the onset) with the 36 / 40 layer in between. Nothing during the hold (the 0.2 s before the go-to, below
  100 m) and nothing after 330 ms; the bail runs give the same shape. Tools: `goto_capture.py`, `align_compare.py`,
  `align_compare_bail.py`.
- **So retail's static is the record itself:** sk8_menu 58 and 59 are 52 ms / 46 ms white-noise bursts (spectral
  flatness 0.32 / 0.41, centroid 6.1 / 6.9 kHz) at gain 0.6288, one at the jump and one 270 ms later, around the
  36 click and the 40 swoosh-thud. Our render of `cellphone_goto_marker` (the port as it is) has the same two
  bursts in the same shape (above 4 kHz −46 / −40 dB; 36 and 59 land ~20 ms later after 58 than in the capture,
  within the data test's 50 ms).

**Change:** none. Porting another sound would be a guess: nothing in the code or the recordings plays one.

**Open (needs the user):**
- Where the user hears the static (the console, the recomp, a video) and whether it is during the hold (while the
  screen static builds up), at the jump, or longer than ~0.3 s.
- Whether our go-to sound plays in their game at all: with `SKATE_AUDIO_TRACE=1` the log shows
  `AUDIO_NATIVE frontend 7F135F9FD28F7F21 sk8_menu 236` on each return. If it doesn't, the static they miss is this
  record (activate and place use the same two noise samples, which may be why those sound right).
- Masking: in retail the bursts stand 15 … 18 dB above the bed above 4 kHz; our bed's high band was measured
  louder than retail's earlier (seams up to +22 dB while grinding, session review 2026-10-03), which could cover
  them in our game. Not measured for this case.
- The recomp's long holds (> 100 m, up to 1 s) are unrecorded; by the code nothing audible depends on the
  distance.

Credits: the recomp (skate3recomp, rexglue, Xenia) as the research build; our own code and words.

### Follow-up 2: the far return (2026-10-04; resolved in Follow-up 3)
The user's answers to the open questions above:
"1. on console it last for about a second and it only happened when you had traveled farther away from where the
marker was set.
2. the go to sound? as in the sound when you return to the marker? Yes it does.
3. maybe"
(1 = where / how long, 2 = whether our go-to sound plays, 3 = whether our riding bed may mask the bursts.)

**Code (the recomp, TU3; reference only).**
- The hold scales with the distance to the marker (`sub_82898FC8`, already in `session_marker/state.rs`): ≤ 0.5 m
  nothing, ≤ 100 m 0.2 s, 100 … 1000 m `d / 1125 + 1/9` s, ≥ 1000 m 1.0 s. The screen static (`cMsgTeleportEffectAmount`)
  ramps 0 → 1 over the hold, so a far return holds the static for up to a second; a short one for 0.2 s.
- The teleport request (`sub_825582D0`, type 3) reaches the game state's message handler `sub_82709740`, which calls the
  decision `sub_82706F50`. It asks `sub_82864C40` whether the destination is streamed in (two world-streamer queries
  `sub_82478D00` around the destination, radius 30 m, or 50 m with the request's flag, plus two streamer state checks).
  Streamed in: the skater is placed at once (`sub_824787E8`) and a message `0xCA1D598F` is posted. Not streamed in: the
  destination is stored and the **loading state** is entered (state function `0x82707508`, event 7 pushed to the state
  event queue `[0x830CFE2C]`), i.e. a loading screen. Which returns load is decided by streaming, not by a fixed distance.

**The user's recording `audiox_marker_20261004_123843`** (couch launcher `recomp-marker`, 41 MB, 0 malformed): two
go-tos.
- Far return (trace 193149.7 ms): the recomp shows the **Loading… screen** (shot 194914). The go-to record plays as on
  short returns (58 at +110 … 150 ms in the capture, 59 at +270 … 280, each ~ −40 dB total, −43 … −46 dB above 4 kHz), then
  within ~10 ms at ~+420 ms the whole mix is cut (−35 → −62 dB, then a tail decaying to −110 dB) and stays silent for the
  loading screen; the new place's ambience starts at +3381 ms (AMBST / WPSET). No SPLC or POST in the window other than
  the record, and no stream start (only the running music stream's reads and, after the load, the new ambience); nothing during the hold (−1.2 s … 0 at −58 … −66 dB above 4 kHz, the ride).
  The audio objects keep updating during the load (GREC / SKID lines continue), only the output is silent.
- Short return (223139.7 ms, camera jump 83 m, no load): the record over the ride, as in the earlier 23 go-tos.
- So in the recomp a far return is: the hold's visual static (up to 1 s), the record's two noise bursts, then silence
  through the loading screen. No ~1 s static sound (wrong, see Follow-up 3: the hold's Treatments crackle was in this
  capture too, below 4 kHz and without POST / SPLC lines). The recomp is not the console: its loading finishes faster and its
  audio output stalled during the load (the capture got ~0.5 s of frames over 1.5 s of trace time), so what the console
  plays while it streams cannot be read from this capture.

**Hooks for the next recording** (recomp `src/research/hooks_marker.cpp`, category `audiox`, so the couch launcher's
`recomp-marker` entry records them; built 2026-10-04 12:44, not yet seen firing): TPMARK (request: distance, hold time,
destination), TPDEC / TPSTREAM (load or not), TPFX (the static amount per tick = the hold's real length), FEREQ (every
front-end sound request, including Lua's), GSTATE / GEVENT (state changes and events), HUBMSG (posted messages, ≤ 2 / s
per id). Field counts and the analysis: local research tools (not published).

**Change:** none yet. Open: what the console plays for "about a second" on a far return: the static during a 1 s hold
(no sound for it in the code or the recomp), the record's bursts followed by the loading silence, or a console-only
effect of the streaming load. Our engine has no streaming load, so a far return never goes silent after the record.

### Follow-up 3: the far-return noise (2026-10-04, ported, branch `audio/respawn-marker`)
User, after a recording with the marker hooks: "it played the noise! its the first return in the recomp run i just
finished"

Listening (user, 2026-10-04, after a play session with the ported crackle): "it sounded great".

**Problem.** On a far return retail plays a noise for about a second that our engine did not play. Follow-ups 1 and 2
found no sound for it: they searched the front-end requests, the SPLC / POST / stream lines and the capture above
4 kHz, and this sound shows in none of them.

**What it is.** The skater's **Class_Treatment** plays it, not the front-end and not the loading screen. Class_Treatment
is the Treatments controller's packet (bank `Treatments`), the same object that plays the pre-landing treatment. While
the screen static is on, its program plays a **teleport crackle**: short samples from slots 1–12, one every 30–130 ms
for as long as the hold lasts, then slot 0 at the jump. The packet is posted once per life and held, so its program
starts these voices itself, with no POST or SPLC per crackle. That is why the earlier searches missed them.

**Evidence (the recomp, TU3; reference only).**
- **The recording** `audiox_marker_20261004_125320` (123 MB trace, 0 malformed, all marker hooks firing) has three
  teleports. One is the Challenge Map teleport at 46670.8 ms (`TPMARK` lr `82864A08`; destination streamed in). Two are
  marker returns: 1559.3 m (hold 1.0 s, request at 298846.2) and 592.6 m (hold 0.638 s, request at 435268.7). Both
  marker returns loaded (`TPSTREAM` 0, `TPDEC` load 1), the 593 m one included.
- **The 1559 m return** (the one the user heard): 61 `TPFX` messages ramp 0.017 → 1.0 from 297852.4 ms. Treatments
  voices (PLAY lines resolved to the bank) play slots 2, 10, 7, 3, 6, 2, 12, 10, 5, 8, 4, 12, 2, 10, 6, from 40 ms
  after the first message up to 811 ms. Slot 0 follows 5.5 ms after the go-to request, and the record and then the
  load's mute (+420 … +2150 ms) come after it. Voice peak gains are 0.02–0.28 (slot 0: 0.1229). The capture over the
  hold: the 250–4000 Hz band rises 12–15 dB above the standing bed (−50 dB) to about −35 dB and peaks at 600–750 ms.
  Its centroid is ~1.1 kHz and almost nothing is above 4 kHz (−65 dB): a low-mid crackle, not white noise.
- **Every hold has it and nothing else does.** All Treatments slot 0–12 voices in the session fall inside `TPFX`
  episodes:
  - the 593 m return: 6 crackles in 0.64 s, slot 0 at gain 0.0404, masked here by the ride (bed −28 dB);
  - a hold released at 0.70 before it (433275 ms): 7 crackles, no slot 0.
  Three earlier sessions (`audiox_bail_20261003_094336`, `audiox_20261003_095054`, `audiox_marker_20261004_123843`)
  have it on all 10 go-tos: 3–5 crackles in each 0.2 s hold, 9 in the earlier far return, and slot 0 5–33 ms after
  every go-to (gain 0.1229 on 10 of 12). Over 68 crackles the peak gain has median 0.1870 and max 0.2842.
- **So it plays on every return and lasts as long as the hold:** 0.2 s up to 100 m, `d / 1125 + 1/9` s up to 1000 m,
  1 s beyond. That matches the user's "about a second … only when you had traveled farther away". On short returns
  its 0.2 s sits under the go-to record. On the 1559 m return the skater was standing and the bed was quiet.
- **Code.**
  1. `cMsgTeleportEffectAmount` (`0xFAF37902`) reaches the VisualDirector's handler `sub_827A9C60`, which stores the
     amount at `+48`.
  2. The presentation builder `sub_827AAF10` writes that amount into the presentation packet (header slot 3) when it
     is ≥ 0 (`0x82165A10` = 0.0), then resets it to −1.0 (`0x8216DEE0`) after every build. So the field is present only
     on frames that received the message.
  3. The packet decoder `sub_827AB790` writes the field into the presentation block: `B+16`+148 = present, +152 = the
     amount. `B = *(*(0x83083C38)+0x2FCB4)`, and `sub_827AB6E0` resets both. These are `B+164` / `B+168`, the two
     words Class_Treatment's update `sub_824DD6F0` reads: `B+164` → w12 = 1, w13 = trunc(`B+168` × 10000); otherwise
     w12 = 0 and w13 keeps its value.
  4. The Treatments program does the rest. The treatment port has had these words since it was written, but the
     source was not known then, so they stayed at their reset values.
- **Trigger condition: the hold itself** (teleport effect amount > 0, message present). It does not depend on
  distance, the load or streaming: distance sets only the hold's length. The load (`sub_82864C40` says "not streamed
  in" → loading state `0x82707508`) is a separate thing that mutes the mix after the record in the recomp. Our engine
  has no streaming load, so nothing maps to it.
- **No Lua.** The only `FEREQ` around both returns are the activate (`82668984`) and the go-to (`828994C4`) requests.
  Lua's front-end requests (through `sub_825A2BC8`) appear elsewhere in the session (menus), never near a return.

**Change.**
- **Engine** (`crate::ui_audio`): a new resource `TeleportEffect` holds retail's teleport effect amount for the
  frame: the engine's (`engine`) and a mod's (`from_mod`, which lapses); `amount()` is the larger of the two.
  `session_marker` publishes the hold's progress into it every frame (`publish_teleport_effect`: progress > 0 on the
  hold's UI ticks and the three from the relocation, as PlayerUI sends the message).
- **Audio** (`game_audio/native.rs`, `player_audio.rs`): each pass hands the amount to `PlayerAudio::teleport_effect`,
  and Class_Treatment's update gets `TreatmentGlobals { flag_164, value_168 }` from it (`B+16` / `B+24` stay at
  reset). The crackle itself is data: the Treatments bank's program, already exported by setup. No new bank, record or
  export.
- **The screen static** (`session_marker/effect.rs`) draws the larger of the hold's amount and a mod's, so both readers
  of retail's message see the same value.
- **Cadence.** The amount is the last UI tick's value (60 Hz UI clock, the hold's existing `state.rs` timing), read
  once per audio pass. Its value between ticks holds, so the flag cannot flicker at high frame rates. At the console's
  30 fps every frame receives a message, and our pass reads it the same way.
- **Mods:** `sdk.audio.teleport_effect(amount)` (command `audio_teleport_effect`, 0..=1, validated) drives the same
  resource. It holds for four UI ticks, so a mod sends it every frame for as long as it should last. 0 clears it, and a
  stopped or disabled mod's amount lapses by itself. Mods already see the marker's actions (`on_event {name =
  "session_marker"}`). Replacing the crackle's samples or program (the Treatments bank) is content-overlay work for
  the audio modding (doc 16, former #36, now part of #32).
- **Bench hook:** `E2E_TELEPORT=<row>,<ticks>` makes the e2e harness play a hold from that row (unset: unchanged).

**Files.** `crates/skate-game/src/{ui_audio.rs, session_marker/mod.rs, session_marker/effect.rs}`,
`crates/skate-game/src/game_audio/{native.rs, player_audio.rs, e2e.rs}`, `crates/skate-game/src/modding/{mod.rs,
audio.rs}`, `crates/skate-mods/src/{vm.rs, audio.rs, api.lua}`, `sdk/skate.lua`,
`crates/skate-audio/src/player/treatment.rs` (doc comment), `crates/skate-audio/tests/player_tricks.rs`, this doc and
spec §11.

**Verification.**
- `teleport_crackle_follows_the_recomp` (data-gated, through the real Treatments bank) runs a 1.0 s hold with four
  seeds. In every run the crackles start within 70 ms (ours 0–33 ms; the recomp 24–68 ms), 15–17 of them sound (the
  recomp: 15), the last lands in the final 40 % of the hold, none comes after the jump, and slot 0 sounds once, 0–17 ms
  after the jump, at 0.1229 (the recomp: 0.1229). The crackles' median peak gain is 0.1889 (the recomp 0.1870) and the
  max 0.2740 (the recomp 0.2842). A 0.2 s hold gives 4 crackles plus slot 0; a hold released at 0.70 gives crackles
  and no slot 0.
- **Level in the game's chain against the capture.** A standing scenario rendered by the e2e harness with
  `E2E_TELEPORT=120,60`, minus the same render without it, folded like the capture (0.4 · (L + Ls + 0.5 C)). Mean over
  the hold, ours vs the recomp (bed subtracted): all bands −41.1 vs −39.8 dB, 250–1000 Hz −45.9 vs −44.9,
  1–4 kHz −43.0 vs −41.5, 4–12 kHz −68.5 vs −66.6, so within 1.3–1.9 dB. The envelope is the same: it builds over
  the hold, peaks at 600–750 ms and ends with slot 0's burst at +1000 ms (4–12 kHz −52 vs −51 dB). A local tool,
  not published.
- The headless e2e bench (scenarios and real sessions, row and fps300, 84 outputs) is byte-identical to the branch
  before this change: the harness has no teleport unless `E2E_TELEPORT` is set.
- Unit tests: the resource (larger amount, clamping, a mod's lapse), the mod command's validation.

**Open.**
- The load's mute (the recomp silences the mix from ~+420 ms until the new place streams in) is not ported: our engine
  has no streaming load. If a load or fade screen is added, retail's loading state is where the mute belongs.
- The second return's slot 0 played at 0.0404 instead of 0.1229 (1 of 12); not explained (MixMap state during the
  ride?).
- `B+16` / `B+24` (another presentation field Class_Treatment reads as w11) are still at reset; that field's message
  is not identified.
- Relative level: in the recomp's capture the crackle sits +6 dB above the go-to record's energy. Our bare runtime has
  them about equal, but it has neither the capture's fold nor the buses. The crackle matches in the game's chain (above).
  The record was not re-measured in that chain.

Credits: the recomp (skate3recomp, rexglue, Xenia) as the research build; our own code and words.

## The car alarm trigger (2026-10-04, follow-up to the world follow-ups)
**Problem.** The car alarm sound was ported (`VehicleAlarm { vehicle }` = horn state 6 for 8 s, the mod event
`alarm`), but nothing set it off the way retail does: a parked car's alarm goes off when the player runs into it.
The user's account (recomp session 2026-10-04 16:11): the user found a taxi and skitched on it; it
ended up parked at the side of the road, and when the user ran into it the car alarm went off.

**Root cause.** The trigger is traffic AI, not audio: it lives in the vehicle's collision callback and its
`StayingParked` state, which the engine does not have (there is no traffic yet).

**Evidence (retail, from the TU3 recompilation as reference and that session; spec
`audio-specs/world-traffic-audio.md` "Car alarm trigger").**
- The vehicle's collision callback `sub_82C3C150` (interface at vehicle `+136`): only while the car is in
  `StayingParked` (`+3424` bit 0x80, set by the state's begin `sub_82C39120`, cleared by its end `sub_82C391F0`),
  and only when the contact message's vector (`msg+48`) is longer than `livingworld_vehicle_characteristics`
  field `543475921FD9E04A` (0.1, no shipped override), it sets the alarm flag (`+3424` bit 0x10) and zeroes the
  alarm timer `+3716` and the parked timer `+3712`. It does not test who hit the car.
- `StayingParked`'s update `sub_82C39138` counts the alarm timer while the flag is set. `StopAlarming`
  (`sub_82C3A4D0`) ends the alarm when the timer passes field `E199FC7CEA222809` (8 s); its action
  `sub_82C3B4E8` clears the flag. Pulling out (`sub_82C3A3A8`) is blocked while the alarm sounds.
- Session: the taxi pulled over and parked at 249.8 s. The user rolled up to its rear bumper at 0.2–0.3 m/s (no
  alarm), stepped off at 255.90 s and ran into it. The alarm post came 130 ms later (256.032 s), and nothing else
  was posted for the car (no impact sound). The alarm then ran for the remaining 64 s, because the user stayed at
  the car (on foot beside it, skating along and on it): every contact restarted the 8 s.

**Change.**
- Engine-facing (`crate::world_audio`): component `VehicleParked` (retail's StayingParked); message
  `VehicleImpact { vehicle, by: ImpactSource, impact: Vec3 }` (the callback's message; `VehicleImpact::speed` for a
  length only); resource `CarAlarmRule` with `AlarmTuning { enabled, min_impact, seconds }` (setup export first,
  then an override from a mod or engine code); read-back message `VehicleAlarmStarted { vehicle, by, restart }`
  for the future traffic AI (restart its parked timer, don't pull out while the alarm sounds).
- `game_audio/car_alarm.rs`: the rule (parked, `|impact| > min_impact`, enabled → `VehicleAlarm` +
  `VehicleAlarmStarted`; several contacts in one frame start it once).
- Timing: the bridge holds horn state 6 for `AlarmTuning::hold_seconds()` = whole console frames past `seconds`
  (retail adds the frame time each AI update and stops at the first update past 8 s: 241 frames at 30 fps,
  8.033 s), the same at any engine frame rate. A repeat `VehicleAlarm` / impact restarts it (retail's timer reset).
  This also applies to the existing `alarm` event (8.000 → 8.033 s).
- Data: setup export `world_tuning.vehicle_alarm` = `{min_impact, seconds}` from the vehicle spec `default`
  (+ `specs` for any spec that differs; none in retail) in `tools/asset_pipeline/world_audio.py`. Installs from
  before it use the engine's constants, which are the same values (`ALARM_MIN_IMPACT`, `ALARM_SECONDS`).
- Mods (API 2, world audio extension 1, backward-compatible): `sdk.world_audio.event(key, 'impact', {speed=m/s,
  source='player'|'character'|'vehicle'|'object'})` on a mod car; traffic option `parked` (default: parked while
  the mod doesn't update the car, as before); `sdk.world_audio.alarm_rule{enabled=, min_impact=, seconds=}` (fields
  replace the rule's numbers for every car; `alarm_rule()` = retail; cleared when the mod stops);
  `sdk.world_audio.read(key).alarm` = seconds left while it sounds. The dev test mod (`mods/world-audio-test`)
  marks its taxi `parked` and reports an impact every frame the skater touches a car's box: run into the taxi and
  its alarm goes off; the moving cars ignore it. The readout shows `ALARM carN s`.
- Engine traffic (when it exists): add `VehicleParked` while the car is in its parked state, send `VehicleImpact`
  from its contact handling (any collider, every contact), and react to `VehicleAlarmStarted`.

**Verification.** See "Proofs and tests (car alarm)" below.

**Research hook (run 2026-10-04 18:03).** `VEHHIT` (every callback: flags before / after, the vector and its
length, the contact point, the other object, the timers, the message bytes), `VEHALARMSTOP` (the alarm's end with
its timer) and `VEHPARK` (StayingParked begin / end), category `traffic`, in the recomp fork's
`src/research/hooks_traffic.cpp`. Session `all_20261004_180303` (0 malformed; that build logged the timers before
the call and gated lines at 100 ms per car unless the flags changed): a taxi (`47CB6660`) parked at 623.7 s and
stood still at (103.81, 34.03, 200.33) to the end of the session; the user walked into it, hit it fast on the board
(three times, with bails), rolled up to it slowly and stayed around it. Player state from `SKATEB` (the player's
board `469434F0`: contacts, deck velocity, 4 Hz) and the screenshots; `msg+64` names the player's part
(`469434F0` = the board, `4693BEB0` = the skater's body, the same `msg+76` = `40451020` for both).

| Time (s) | Action (shot) | Part (`msg+64`) | Other's speed | Car | \|v48\| | Flags |
|---|---|---|---|---|---|---|
| 630.094 | on foot, walks into the rear with the board in hand (629.4) | board | 2.1-2.3 m/s (walking) | 0 | 2.652 | 80 → 90 (alarm) |
| 630.312 | the same walk, the body follows | body | walking | 0 | 3.603 | 90 |
| 644.508 | rides into the rear at 7.9 m/s, bails onto the boot (645.9) | body | 7.93 m/s (84 ms before) | 0 | 5.788 | 80 → 90 (alarm) |
| 645.5-648.7 | lies on the boot (resting contact) | body | ~0 | 0 | 0.003-0.07 | 90, timer runs on |
| 650.3-652.4 | rolls up at 1.1-1.6 m/s, stops short (652.1), steps off | - | - | - | no call | - |
| 652.567 | the board swings into the bumper as it is picked up | board | ~4.6 m/s | 0 | 6.466 | 90, restart |
| 666.736 | rides in at 8.5-9.4 m/s | body | 9.39 m/s (134 ms before) | 0 | 9.068 | 80 → 90 (alarm) |
| 677.795 | a pedestrian (`47BAC700`, `PEDXYZ`) walks along the side | ped | 0.3-0.5 m/s | 0 | 0.225-0.99 | 80 → 90 (alarm) |
| 721.732 | rides in at 8.7 m/s, bails beside the car (722.9) | body | 8.70 m/s | 0 | 6.698 | 80 → 90 (alarm) |
| 722.06-723.37 | the riderless board rolls along the car, deck touching | board | 0.074-0.083 | 0 | 0.073-0.084 | 90, timer runs on (0.07 → 0.90 s) |
| 723.53-725.34 | the same board speeds up | board | 0.18-0.41 | 0 | 0.150-0.401 | 90, restart each call |

Same-moment pairs of the riderless board's deck speed (`SKATEB`) and \|v48\|: 0.0736 / 0.0737 (723.368),
0.3313 / 0.3335 (725.120), 0.0783 / 0.0782, 0.2533 / 0.2590, 0.0700 / 0.0667, 0.2512 / 0.2635, 0.4099 / 0.4012.

**Answers.**
- `msg+48` is the relative velocity at the contact in m/s, the car's minus the other body's (a board rolling +x
  gives -x, a pedestrian walking -x gives +x): its length equals the other's speed against the parked car to
  0.01 m/s for a few-kg board, and a ~70 kg pedestrian at 0.3-0.5 m/s gives 0.2-0.5. An impulse would scale with
  mass, and a body lying on the boot would carry its weight (tens of N·s per step); it gives 0.003-0.07. Not a
  penetration depth either (it follows the speed while the contact stays the same). The port's reading (a speed,
  `min_impact` 0.1) is right; nothing changes. The strict `> 0.1` shows too: 0.074-0.084 m/s against the car let
  the alarm timer run on, 0.150 restarted it.
- Every body reaches the callback: the board (carried, swung, riderless, during a bail), the skater's body and a
  pedestrian; who hit the car is not tested (the pedestrian set the alarm off). A slow roll on the board did not
  touch the car in this session (it stopped short and the user stepped off), so the 16:11 session's 0.2-0.3 m/s
  roll without an alarm stays unexplained: either no contact (the board stopped at the bumper's lip) or the car
  was not yet in StayingParked; the measured rule would have fired at that speed.
- The alarm's end: all 6 `VEHALARMSTOP` lines come at 8.0333 s (241 console frames), as the port holds it. At
  the end the state machine leaves StayingParked and re-enters it with the parked timer at 0 (`VEHPARK` 0 with the
  flags `10`, then 1 with `80`), so every alarm restarts the parked time: the taxi stayed parked for the remaining
  110 s while other cars pulled out after 20 / 30 s. The future traffic AI does this from `VehicleAlarmStarted`.
- Three contacts with no other object (`msg+64` and `msg+76` 0) gave 2.2-5.9 during the walk and two hits; not
  identified (probably a part with no owner); they changed nothing because the alarm was already sounding.

**Open questions.**
- Per-spec values: none shipped; the export keeps a `specs` map in case, the engine uses one rule for all cars.

### Proofs and tests (car alarm)
- Local player byte-identical: the e2e bench (13 scenarios + 8 sessions, `row` and `fps300`) from this branch's
  head before the change (`a0`) against the result (`a1`): all four sets IDENTICAL (26 + 26 + 16 + 16 outputs).
- New unit tests: `game_audio::car_alarm::tests` (5: the retail conditions incl. the strict `>`, the hold in whole
  console frames, impact → alarm with restart and the 7.9 s / 8.1 s edges, the same length at 30 / 60 / 144 fps,
  overrides and disable), `modding::world_audio::tests` (2: `impact` reaches the rule, `alarm_rule` merges and is
  cleared with its mod), the setup export (`world_tuning_reads_the_setup_export`, Python
  `test_vehicle_alarm_reads_the_default_spec_and_overrides`), skate-mods option / command validation and the test
  mod run (exactly one impact per frame while the skater is in the taxi's box). Against the real install's
  collections the export gives `{min_impact: 0.1, seconds: 8.0}` = the engine's constants.
- Suites (worktree target, private-data env): release build ok; skate-audio 235 + integration pass, all its
  ignored data tests pass; game_audio 49 pass, its 35 ignored data tests pass; Python `test_world_audio` 9 pass;
  skate-mods passes except the known `skyline_every_component_is_real_and_drives_through_ground_contact` (gitignored
  model missing in a fresh worktree).
- Not done: an in-game listen (the user: enable `dev-world-audio-test`, run into the parked taxi).

### Files (car alarm)
- `crates/skate-game/src/world_audio.rs` (`VehicleParked`, `ImpactSource`, `VehicleImpact`, `VehicleAlarmStarted`,
  `AlarmTuning`, `CarAlarmRule`, `ALARM_MIN_IMPACT`), `game_audio/car_alarm.rs` (new), `game_audio/world_bridge.rs`
  (the hold, `Bridge::alarm_left`), `game_audio/world_sources.rs` (`vehicle_alarm` export), `game_audio/mod.rs`.
- `crates/skate-game/src/modding/{mod.rs, world_audio.rs}`, `crates/skate-mods/src/{world_audio.rs, vm.rs, api.lua}`,
  `sdk/skate.lua`, `mods/world-audio-test/main.lua`.
- `tools/asset_pipeline/world_audio.py`, `test_world_audio.py`.
- Specs: `audio-specs/world-traffic-audio.md` "Car alarm trigger", `npc-livingworld-re.md` §6,
  `world-audio-hookin-spec.md` (the message table).

## Open questions before P3 (kept for the record)

- **Speech playback** in the host: the manager, the library and the streams. The level and pan
  mapping is still open.
- **The NPC instance's components.** G3 (2026-10-03) settled which ones run for an NPC:
  - Wheels runs (spin streams on layers 0 / 1; layer 2 and the post-start parameter writes are
    local only);
  - Clothing runs (`sk8_foley` 73 / 74; body slide / cloth falls on bails; the start-block float is
    0 for non-local; the eq-chain pick takes `local72`);
  - Tricks and Treatment are local only;
  - OffBoard creates its footstep packets but plays no steps for NPCs;
  - the bail grunt `sub_824BF5F8` posts speech.

  Instance 1 changes hands often (12 times in 74 s), so claims and releases must stay cheap.
- **PedBodyFall** (decode the object), **Tazer** (needs AEMS op 38).
- **Traffic bindings:**
  - the `TrafficCarPhysics.in0` writer (|v_car − v_listener| clamped to 35, slewed by 100 /s,
    × 32767 / 35; opens the A11 near boost);
  - the 3DObjPos 4.1 / 4.2 points (front and rear ±1 m along the heading, R+80 / R+96, likely);
  - retail's frozen record when a held car leaves the 40 m list (recorded behaviour, not ported).
- The per-model tables from `aud_characteristics` (shoe class, kind, far threshold 20 / 30 m) as a
  setup export, so the engine names a model and gets the rest. The horn kinds per model are open.
- Moving process / update into `mixmap_frame` (retail's split; the inputs are one console frame
  old today).
- The wider mod surface (P4): content overrides (banks, samples, programs, speech lines), mod
  emitters on the native voice graph, tuning read / write, `sdk.audio.post`, and audio event hooks.
