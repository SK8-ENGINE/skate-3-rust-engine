# Hooking up world audio (traffic, pedestrians, NPC skaters)

Part of PR #32 (branch `gameplay/audio`; doc [11](11-audio.md) has the ported audio itself).

## Problem

Skate 3's living world has its own sounds: traffic engines with Doppler, horns, skids and car
alarms; pedestrians' footsteps and speech; and the board sounds of an AI skater near the camera. The
native audio port (`crates/skate-audio`, `skate_audio::world`) already plays all of them the way
retail does. But the engine has no traffic, pedestrian or AI-skater system yet. Nothing upstream or
in any fork has one, and no issue asks for one (prior-work check, 2026-10-03). The sounds therefore
had no way in.

The goal (the user, 2026-10-03): "build out everything that can then easily be hooked to by the game
engine once they are added", in this PR, and moddable like the rest of the engine.

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
| `engine` | vehicle record `+168` | the `aud_traffic_engine` record: sedans / hatchback `c01_family01`, sports / muscle `c03_sports01`, taxi / patrol `c04_taxi01`, SUV / pickup / minivan `c05_truck01` (entity → vehicle spec → record, G1). The engine's patch override picks c06 / c07 / c08 itself. | the vehicle system, from the model | — (an unknown name is logged once and stays silent) |
| `speed` | `+148` | m/s | the vehicle system | the length of the velocity |
| `load` | `+144` | the driver's signed acceleration in m/s². G1 measured −15.6 in a hard stop, up to +3 pulling away, and 0 while cruising. It goes into the engine and skid words × 3000. | the driving AI | derived from the speed change |
| `horn` | `+156` | `None`, `Honk(1..=5)` or `Alarm` | the AI (honk decisions) | `None` |
| `skidding` | `+160` | the tyres skid | the AI (hard braking or a swerve) | false |

### `PedAudio` (retail `SFXObj_PedestrianSFX` / `PedestrianSpeech`, ped state S, G2)

| field | retail | meaning | default |
|---|---|---|---|
| `voice` | `S+84` | the model = the speech voice id 41–96 | none (no speech) |
| `shoe_class` | `S+132` | 1–5 per model (`aud_characteristics`). Most models are 2; no model uses 1, which is silent. | 2 |
| `weight` | `[obj+28]+144` | 1–5. It changes per ped in retail (1 beyond about 12 m, 2–5 nearer) and its meaning is open. | 1 |
| `close_range` | `S+96 == 64` | the security guards' close-range footstep levels | false |
| `feet_down` | `S+74` / `S+73` | the walk animation's foot plants (A, B) | up |
| `foot_materials` | `S+140` / `S+144` | the audio surface materials under the feet | 0 (retail's pavements read 0 in every line) |
| `footsteps_on` | `S+68` | footsteps on | retail's rule: the 3 nearest peds in the list |
| `speech_distance` | `S+148` / `S+156` | distance to the listener / the model's far threshold. Beyond the threshold the far `_f` lines are used. | the 3-D distance / 20 m (retail for regular peds) |
| `speech_value` | `S+136` | the state graph's speech value | 0. Send `PedSpeechEvent` rather than writing it. |

### `NpcSkaterAudio` (the MixMap Player slot's second instance)

| field | meaning | default |
|---|---|---|
| `list_order` | the skater list position (retail walks its list in order) | spawn order |
| `state` | this frame's `AudioState`. A skater simulated with the player's physics uses `game_audio::skate_events::skater_audio_state(physics, skater, &mut memory, dt)`, the same builder the local player uses (proved identical, below). Anything else uses `AudioState::rolling(&LiteSkater { .. })`: speed, wheels, materials, grind / air flags. With that fill, rolling, surfaces, seams, grinds and landings sound; tricks and foot / body foley stay silent. | none (not published) |
| `remote` | a remote multiplayer player (see below) | false |

### Messages, resources and the read-back

- `PedSpeechEvent { ped, value: SpeechValue }`: sets the ped's speech value, so PedestrianSpeech sees
  the change and requests a line. `SpeechValue::from_name` accepts the state-graph names (`DoWarning`,
  `LongCheer`, …), the short names `warn` / `cheer` / `slam` / `flee` / `knockdown` / `nearby`, and
  numbers.
- `VehicleHorn { vehicle, kind, seconds }`: holds horn kind `kind` for the caller's time. The length
  is the AI's choice, not retail data.
- `VehicleAlarm { vehicle }`: retail's alarm, horn state 6 for **8 s**.
- `LivingWorldAudio { expected }`: set it at map load when the system will publish. The 13 world
  banks then decode on the prefetch worker instead of the game thread.
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
commands.spawn((transform, PedAudio { voice: Some(59), shoe_class: 5, ..Default::default() }));
speech.write(PedSpeechEvent { ped, value: SpeechValue::WARN });

// An AI skater simulated with the player's physics, near the camera:
npc.state = Some(skate_events::skater_audio_state(&physics, &skater, &mut memory, dt));
```

## Mod surface (`sdk.world_audio`, API 2, capability `world_audio` = 1)

Mods publish through the same components: each key becomes an entity. The rules follow the existing
mod rules. Keys belong to the calling mod. The limits are **16 objects per mod and 64 in all**.
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
sdk.world_audio.event('ped1', 'speech', {value='warn'})        -- name or number
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
  `sk8_foley` step ids play. A gait clock alternates the feet; it is dev-only, not retail. Shoe
  classes are 2–5, and the voices cycle through 41–96. Reactions:
  - warn (11) when you pass within 1.5 m at more than 3 m/s;
  - cheer (23) on a landed trick within 10 m;
  - slam (25) on a bail within 10 m.

  Speech requests are logged (`AUDIO_WORLD speech`). Playback comes in the next pass.
- **A ghost NPC skater** replays the 20 s window of a recorded state log, 5 m beside the start.
  Settings: `ghost_log`, `ghost_from`.
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

## Open questions and the next pass (P3–P5)

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
