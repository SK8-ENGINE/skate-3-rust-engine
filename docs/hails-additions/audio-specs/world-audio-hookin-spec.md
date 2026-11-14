# World audio hook-in: design spec (2026-10-03)

Goal (user, 2026-10-03): build out everything the game engine can easily hook into once those systems
are added, and make that this PR's goal. The world audio (traffic, peds, ped speech, NPC
skaters' boards) becomes complete and trivially hookable by future engine systems (peds, vehicles, AI skaters) that
the engine does not have yet. It stays in **PR #32** (branch `gameplay/audio`). Implementation comes after the
current code-fix pass (PR #32 readiness plan, step 1), because the same files are touched.

Inputs: `world-traffic-audio.md`, `world-ped-audio.md`, `world-speech.md`, `world-npc-skater-audio.md`,
`npc-livingworld-re.md`, doc 11 "World sound sources" / "NPC skaters' board sounds" / "NPC skaters' second grain bed".
Code read: `crates/skate-audio/src/world/*`, `game_audio/{world_sources,npc_skaters,emitters,native,skate_events}.rs`,
`tools/asset_pipeline/world_audio.py`.

## 1. Prior work (checked 2026-10-03)

| where | what | relevance |
|---|---|---|
| Upstream SK8-ENGINE PRs (#1–#35), issues (#2–#34), branches (`main`, `audio-integration` = main, `skyline-driving-update` merged) | **No ped / traffic / AI-skater / NPC system, open or closed, and no issue asking for one.** `gh search` "npc", "traffic" → only our #32. | Nothing to fit, nothing to duplicate. The API defines the contract. |
| Upstream `skate_data::skate_map::Route { rail, skaters, speed, spacing }` (`.skate` maps) and `skate_world.rs:367` "SKATE LIMITATION: N NPC routes parsed; supplied game has no NPC controller." | The custom map format already carries **AI-skater routes** (a rail, a skater count, speed and spacing). Nothing consumes them. | The likeliest first AI-skater system is "skaters following `map.routes`". The NPC-skater API must work for a route follower. The dev publisher can use the same routes. |
| Upstream `crates/skate-dynamics` (Rapier island: bodies, kinematic proxies, joints) and `mods/Skyline_Drive_Mod` (a Lua-composed, player-driven car with its own WAV engine sound through `sdk.audio`) | Player-driven vehicles only, built in Lua. No traffic. | Traffic cars would likely be kinematic proxies on skate-dynamics. A mod car *could* publish as a traffic vehicle (optional, not retail: question Q5). |
| Upstream PR #15 (laaledesiempre, open) "Dynamic props" | DMO props as live instances with rigid bodies | Adjacent only: DMO impacts are a different audio path (not in this spec). |
| Upstream PR #1 "Mx/audio engine vehicles" (andrewnakas, open) | The diff holds only `skate-audio-core` (a player audio engine). No vehicle files despite the title. | None. |
| Fork `andrewnakas/skate-3-rust-engine` `mx/vehicle` (unmerged, 23 behind) | A **Vehicle SDK**: crate `skate-vehicles` (Rapier 0.35.3), `skate-game/src/modding/vehicles/{mod,audio,engine_sound,network}.rs`, `docs/vehicle-sdk.md`. Lua `sdk.vehicle.spawn/enter/control/tune/read/exit/reset/remove`. Keys belong to the calling mod; at most 8 vehicles per mod, 32 in all; vehicles are removed on mod disable / reload. `read` returns position, rotation, heading, signed speed, phase. Its engine sound "follows speed/throttle" with its own samples. | **Shape to copy for the Lua angle**: keys owned per mod, per-mod / global limits, removal on disable. A drivable vehicle publishes exactly what `VehicleState` needs (position, heading, speed, throttle → `load`). No license upstream or in the fork: describe, don't copy. |
| Other forks (michaelmoskie, patchedsoul, Vorobey1234, jdpc-dev, samwhosung, d4tect, …) | Only `main` / `macos` / `audio-integration` / `skyline-driving-update` / `rtx-path-tracing` branches | Nothing. |
| `chasmlol/skate3clone` (Rust/Bevy), `chasmlol/2010-rust-rewrite-mashup` | No ped / traffic / NPC files in the tree | Nothing. |
| `mchughalex/skate3recomp` (+ ports) | The retail code we read (no license: run / read only) | Our oracle: recorded recomp sessions. |
| Our own (the world / NPC audio plan, the PR #32 readiness plan, doc 11, the notes above) | Foundation built (inert hosts). The readiness plan already lists this work: engine API, complete sources, map-change fix, dev publisher, "hooking up world audio" doc. | This spec. |

**Conclusion:** there is no engine-side representation to fit. The API should be ECS-native (components on the
engine's own entities) so that whatever system arrives (route-following AI skaters, a census / navmesh ped system,
traffic on road graphs, Lua mods) only adds components. It must not need to know about AEMS.

## 2. Inventory

Legend: **done** = ported and harness-tested headless; **partial**; **missing**.

| source | retail object(s) | state | what is missing |
|---|---|---|---|
| Traffic engine | SFXObj_TrafficEngine (4.0), `TRAFFIC_CAR`, `C00`–`C08` by patch | **done**: RPM model (gears, wobble, slew, rise / fall), the patch override, the front / rear split, Doppler through MixMap B0–B2 | **settled 2026-10-03 (§7.3 G1), to port:** the model → record table (entity → vehicle spec → `aud_traffic_engine`), `+112` = heading, `+144` = acceleration m/s², the `TrafficCarPhysics.in0` writer (slewed relative speed / 35, opens the A11 gate). Still open: the 3DObjPos 4.1 / 4.2 / 4.3 binding (front / rear points at ±1 m found, R+80 / R+96) |
| Horns | SFXObj_TrafficHorn, `TRAFFIC_HORN` | **done**: posted once, variant `rand()%9`, kind = horn state 1–5, patch 0 never honks | the AI's honk decision (engine side: blocked lane, the player in the way; `IsBeingHonkedAt` ped+5896) |
| Car alarms | TrafficHorn state 6, `c_car_alarm` (`car_alarms`) | **done** | the trigger (car hit) and the 8 s stop (vehicle +3716) are AI behavior → the bridge offers an `Alarm` event with retail's 8 s (§3.4) |
| Skids | SFXObj_TrafficSkids, `TRAFFIC_SKID` | **done**: plays on the skid flag | the AI's skid flag (hard braking / swerve) |
| Woosh | SFXObj_TrafficWoosh (4.3) | n/a: no MixMap E record writes its outputs, so retail leaves it unused | nothing |
| Doppler / drive-by | MixMap Traffic B lookups | **done** (ObjPos rates) | velocity must be continuous: a teleporting proxy needs `AudioVelocity` (§3.2) |
| Traffic lights | `sub_826B1540` phases 7 / 1 / 0.5 / 0.4 s | no audio object found in the traces | **none:** Skate 3 has no traffic-light or crossing sounds; nothing to build. |
| Instance assignment (4 traffic, 15 peds) | retail's SFX object manager | **partial**: nearest-N (`owners::Pool`) | **confirmed 2026-10-03 (§7.3):** peds = the first 15 of a nearest-first list cut at 50 m (manager `sub_824F2890`); traffic holders were the 4 nearest (horizontal) in 311 / 311 holder-seconds, list cut at 40 m (horizontal). Add the 50 m / 40 m cuts |
| Ped footsteps | PedestrianSFX (5.1): `livingword_footstep` × 2 + `sk8_foley` steps | **done**: 97 % of retail's `fstep_livingworld` starts fall in our sample groups | **settled 2026-10-03 (§7.3 G2), to port:** per model (= voice id) shoe class / kind / type / variant / far threshold from `aud_characteristics`; `+68` = the 3 nearest peds. Still open: `[obj+28]+144` (dynamic 1–5, not a model weight), `sub_824D8E60` flag, the 3 % of slots (w9 / w11 / jump / collision combinations) |
| Ped speech request | PedestrianSpeech (5.0) | **done**: value change → request, 7 / 8 remap, near / far flag. **Not done**: value 49 (`sub_824D9C70`), the value 29 photographer timer | |
| Speech manager + library | `TheSpeechSystem`, `.evt` rules | **done** (`speech_manager.rs`, `speech_rules.rs`); the recomp's take reads agree 57 / 57 clips, 15 / 15 sequences | **not wired in the host**: `world_sources` only logs `AUDIO_WORLD speech …`. The library's request queue (16 slots, 8 streams, priority, timeout) and the interrupt by priority (`sub_824A73F0`) are not ported. **Level / pan mapping unknown** (which PedestrianSpeech output sets the stream's gain). `+148` / `+156` (the near / far pair) = distance to the listener / the model's far threshold (20 m for peds; §7.3 G2). Takes decode only opt-in (`SKATE_SETUP_SPEECH=1`, about 2.4 GB of PCM for the free-roam events). |
| PedBodyFall | 5.2 | **missing** (object not decoded) | decode the object; knock-down trigger from the ped system |
| Tazer | 5.3, `Tazer.abk` `c_tazer` / `c_tazer_grn_play` | **missing**: needs AEMS op 38 ControlClass in the evaluator | op 38; the tazer state from the ped (guards, warn + taze) |
| Ped hand props, ATM, vending, plugins | plugin objects | not looked at | research, later |
| Maincast / cameraman / announcer speech (pro lines around the player, the cameraman's "own the spot") | same library, other managers (`sub_824AC560`) | the `.evt` files parse; not indexed or hosted | outside the ped API; later |
| NPC skater board | Player slot instance 1 (`CSTATEMGR_Player`, the first NPC within 30 m, held until ≥ 30 m) | **done**: `Slots`, every non-local branch of the board components, the soft word, the second grain bed, collisions into the shared manager | the NPC instance's **Wheels spin-down, Tricks, Treatment, OffBoard footsteps, Clothing, board slide** (all per instance in retail); the released instance's grain players; the manager gate (`[0x830670B8+20]` == 5, `sys+560` == 1) |
| NPC skater speech | `sub_824BF5F8` bail grunt (message 8206 / 115 to its `CSTATE_SkaterSpeech`), SkaterSpeech manager `sub_824F7D10` (one record per skater, no distance gate) | **missing** | part of the speech stage |
| NPC skater audio state | `AudioState`, filled like the local player's | **partial**: the host takes a ready `AudioState` | `skate_events::audio_state` is private and tied to the single `GamePhysics` / `SkaterRuntime` resources plus the `Seen` memory: it must become a per-skater function (§3.5) |
| Event PA / crowds / music zones (.ems types 6 / 7 / 4) | Crowd / Speaker slots | not ported | events, challenges and a music player: outside this API |
| Process / update order | retail: process before the tick, update after | **partial**: both run after `native::mixmap_frame`, so the inputs are one console frame old | move into `mixmap_frame` (§3.8) |

### 2.1 The map-change bug (confirmed by reading)

`emitters.rs` (`world_emitters`, on a new `(map.name, map.generation)`) calls `Native::unload_map_banks`
(`native.rs:406`). That unloads every bank except `emitter_utility`, the seams utility bank and the player
(`player_audio::BANKS` / `OPTIONAL_BANKS`). It therefore unloads **all 13 world banks** (`TRAFFIC_BANKS`,
`PED_BANKS`). `Runtime::unload_bank` → `Evaluator::unload_bank` destroys every instance of the bank and drops its
class constructors. The posted **nodes stay in `eval.nodes` (refcount 1) with no instances**: node ids are never
reused (`next_node` counts up), so nothing is corrupted. But:

- `WorldHost.nodes` still maps `(owner, slot)` to those dead nodes. `WorldHost.vehicles` / `ped_objects` still hold
  each owner's `Vehicle` / `PedSfx` with "packet posted". `Engine::process` posts only when it has no packet yet, so
  **an owner that keeps its instance across the change is silent for the rest of its life**: every redeliver goes to a
  node with no instances. This happens whenever ids survive the change, such as a counter restarting at 0, or a
  reused Bevy `Entity`'s bits.
- The `Pool`s keep their holders and the 3DObjPos blocks stay active until the old ids disappear. `host.banks` stays
  `Some(_)`, which is harmless because `ensure_bank` reloads the banks every frame with owners.
- Dead nodes leak until the release that never comes (small).
- `NpcHost` is mostly safe: its posts go into the player banks, which are kept. The old map's skaters vanish, and
  `Slots::assign` releases them. Its `beds` / `objects` / `nodes` are still per old id, and a reused id would carry
  over held state, so it is reset for the same reason.
- `Pool::clear` and `Slots::clear` exist but nothing calls them.

**Fix (Phase 0):** give `Native` a `map_epoch: u64`, incremented by `unload_map_banks`, which already bumps
`prefetch.clears()` (the same idea). `WorldHost` and `NpcHost` remember the epoch they last ran in. On a mismatch,
**before** the frame's work:
1. Release every held node (`rt.release`; harmless on a dead node, and it frees the leak).
2. `Pool::clear` / `Slots::clear`, and deactivate each held instance's 3DObjPos blocks.
3. Drop the per-owner objects and the NPC beds (`grain_bed::stop_npc`).
4. Release the ped Splice steps.
5. Reset `banks = None` and `last_camera = None`.

The banks reload at the next owner, through the prefetch when `expected` is set.

Better still, release **before** the unload: `world_emitters` runs first, so the host would need a hook. The epoch
reset after the fact is correct because releasing a dead node is safe. It is also simpler and keeps the
emitter / world order free. Regression test: publish a vehicle and a ped, step, `unload_map_banks`, keep publishing
the same ids, step → the engine posts again, and voices render again (headless, data-gated like
`world_banks_are_prefetched…`). Also check the reset on `SKATE_AEMS_WORLD=0` and with zero owners (no work).

Also note: the hosts compute `dt` from `m.ticks - last_tick` (`CONSOLE_DT × evaluations`). The current fix pass
reworks the host clock (pr32-ready step 1: host clock = physics steps), so Phase 0 re-checks this arithmetic after
that lands. A MixMap rebuilt with `ticks = 0` would underflow `m.ticks - host.last_tick` (u64); reset `last_tick` with
the epoch, and use `saturating_sub`.

## 3. The hook-in surface

### 3.1 Principles
- **Engine systems only add components to their own entities** and send a few events. They never see `WorldOwners`,
  `NpcSkaters`, NodeIds, MixMap keys, packets or banks. The current resources stay as the internal seam. A bridge
  system fills them from the components each frame. This keeps the tested hosts unchanged.
- **Retail semantics are kept:** values the retail objects read as *state* (horn state, skid flag, speech value) are
  per-frame component fields. Events are only conveniences that write the state for a retail-defined time (alarm
  8 s) or once (speech value).
- **The audio decides who is audible.** The engine publishes everything it simulates. The fixed retail instance
  limits (4 traffic, 15 peds, 1 NPC skater within 30 m) are applied by the hosts. The engine can read back which
  objects hold an instance (`WorldAudioInstance`) to skip work (§3.6).
- **Lifetime = the entity.** Spawn = insert the component; despawn or remove = release, with no extra call. Ids are
  `Entity::to_bits()` (generation included, so a reused index is a new owner).
- Everything stays inert when no component exists (the current "returns at once" path). `SKATE_AEMS_WORLD=0` /
  `SKATE_AEMS_NPC_SKATERS=0` keep working.

### 3.2 Module and components

New public module `crates/skate-game/src/world_audio.rs` (engine-facing; re-exports only plain types). The bridge
lives in `game_audio/world_bridge.rs`. `skate-audio` stays engine independent: the components are thin wrappers that
convert into `VehicleState` / `PedState` / `NpcSkaterAudioState`.

Position, velocity and heading come from the entity's `GlobalTransform` by default. The bridge derives velocity from
the transform delta over the frame's dt. An optional `AudioVelocity(Vec3)` overrides it (needed for teleports or
kinematic proxies, because Doppler and the ObjPos rates read it).

```rust
// ---- vehicles (traffic) ----
#[derive(Component)]
pub struct TrafficAudio {
    /// The vehicle model's `aud_traffic_engine` record name ("c04_taxi01", …; `world_tuning.traffic_engine`).
    /// Unknown → "default" (patch 2 = silent; logged once).
    pub engine: String,
    /// `+148` speed (m/s, ≥ 0). `None` = |velocity|.
    pub speed: Option<f32>,
    /// `+144` × 3000 → engine w15 / skid w6 = the driver's signed acceleration in m/s² (§7.3 G1: −15.6 hard
    /// stop … +3 pulling away; 0 cruising). `None` = the bridge derives it from the speed change.
    pub load: Option<f32>,
    /// `+156`.
    pub horn: HornState,          // None | Honk(1..=5) | Alarm
    /// `+160`.
    pub skidding: bool,
}
// Heading (`+112`) = GlobalTransform forward: confirmed (§7.3 G1, the world matrix's forward row), no override.
// `engine` can be filled from the vehicle model through the setup-exported entity → spec → record table (§7.3 G1).

// ---- pedestrians ----
#[derive(Component)]
pub struct PedAudio {
    /// Speech voice id 41–96 (the clip names' voice; `speaker_bits` → type / variant bits). None = mute.
    pub voice: Option<u32>,
    /// `+132` shoe class 1..=5 (1 = silent in `fstep_livingworld`); `+96 == 64` (security type). Both follow from
    /// `voice` through `aud_characteristics` (§7.3 G2), so they default from the voice. `[obj+28]+144` ("weight")
    /// is dynamic per ped (1 beyond ~12 m, 2–5 near), meaning open; default 1.
    pub shoe_class: u8, pub weight: u8, pub close_range: bool,
    /// `+73/+74`: the walk animation's foot plants (A, B).
    pub feet_down: [bool; 2],
    /// `+140/+144`: material under each foot (AudioSurfaceMap material; `None` → the bridge looks the
    /// ground up under the foot, or 143 = none).
    pub foot_materials: Option<[u32; 2]>,
    /// `+68`: None = retail's rule (§7.3 G2): on for the 3 nearest peds of the nearest-first list (≤ 50 m).
    pub footsteps_on: Option<bool>,
    /// `+148` / `+156` near / far pair (§7.3 G2): distance to the listener / the model's far threshold (20 m).
    /// None = the bridge computes both (retail).
    pub speech_distance: Option<(f32, f32)>,
    /// `+136`: written by `PedSpeechEvent` (below); the state the footstep packets also read (jump 4/5,
    /// collision 6/7). Engine systems normally leave it to the event.
    pub speech_value: i32,
}

// ---- NPC (AI) skaters ----
#[derive(Component)]
pub struct NpcSkaterAudio {
    /// The skater list position (retail iterates its list in order; spawn order is fine). Set by the bridge from a
    /// spawn counter when 0.
    pub list_order: u32,
    /// The full audio state, filled like the local player's. Only needed while within `AUDIO_RADIUS` + margin
    /// (see `WorldAudioInstance`).
    pub state: Option<skate_audio::player::AudioState>,
}

// ---- read back (inserted / removed by the bridge) ----
#[derive(Component)]
pub struct WorldAudioInstance { pub slot: WorldAudioSlot, pub instance: u32 }   // Traffic | Ped | PlayerSlot

// ---- optional ----
#[derive(Component)] pub struct AudioVelocity(pub Vec3);
```

### 3.3 Resources
- `LivingWorldAudio { expected: bool }`: the living world will publish on this map, so the world banks are
  prefetched (maps to `WorldOwners::expected`). A system sets it at map load / spawner start and clears it when the
  world stops. When not set, the bridge sets `expected` itself as soon as any component exists (no prefetch, the old
  game-thread load).
- `SpeechClock` is not needed: the manager clock is the audio host's time.
- `WorldAudioStats` (debug, read only): published counts, holders per pool, speech requests and refusals. For the
  overlay and logs.

### 3.4 Events (Bevy `Message`s in 0.18)
| event | effect | retail basis |
|---|---|---|
| `PedSpeechEvent { ped: Entity, value: SpeechValue }` | sets `PedAudio.speech_value` → PedestrianSpeech sees the change → manager → line | `SendSpeechEvent speechvalue=N` on state entry (`speech::SPEECH_VALUES`: 10 CollisionNearbyReaction, 11 warn, 20 Flee, 23 LongCheer, 25 StopCheer, …). `SpeechValue` is a typed enum with `Raw(i32)`. |
| `VehicleHorn { vehicle, kind: 1..=5, seconds }` | holds `horn = Honk(kind)` for `seconds`, then back to `None` | horn state `+156`; the duration is the AI's (not retail data: the caller chooses) |
| `VehicleAlarm { vehicle }` | `horn = Alarm` for **8 s** (241 console frames, 8.033 s; a repeat restarts it) | car alarm stops after 8 s (`npc-livingworld-re.md` §6, vehicle +3716) |
| `VehicleImpact { vehicle, by, impact }` | on a `VehicleParked` car whose `impact` is longer than 0.1 (`CarAlarmRule`): `VehicleAlarm` + `VehicleAlarmStarted` | the collision callback `sub_82C3C150` (`world-traffic-audio.md` "Car alarm trigger") |
| `PedKnockDown { ped }` | reserved for PedBodyFall (Phase 3) | not decoded |
| `PedTazer { ped, phase }` | reserved for Tazer (needs op 38) | |
| `NpcSkaterBail { skater }` | the NPC bail grunt (SkaterSpeech message 8206 / 115) | `sub_824BF5F8`, Phase 3 |

Skater-vs-car / skater-vs-ped impacts are **not** a world-audio event. They are the local player's collision
manager's (ported). What retail posts for a car contact (its material / class) is open (§6).

### 3.5 NPC skater state: a reusable builder
`skate_events::audio_state(&GamePhysics, &SkaterRuntime, AudioFrame)` is the only correct way to fill an
`AudioState` (retail bridge semantics: air time, grind family / material latch, the deck and region rings, the step
code, push plant). Refactor (behavior-identical for the local player; prove it with the existing state-log
replays, per the regression check's identical-behavior proof) into:

```rust
pub struct SkaterAudioMemory { /* the per-skater part of `Seen`: air_time, grind_family, grind_material,
    jump_velocity, grind_impact, deck/region rings, step_code, push / plant edges */ }
pub fn skater_audio_state(physics: &GamePhysics, skater: &SkaterRuntime, memory: &mut SkaterAudioMemory, dt: f32)
    -> AudioState;
```

An AI-skater system that runs its own `GamePhysics` / `SkaterRuntime` per skater (the obvious way to make AI
skaters skate like the player) calls this once per frame, only for skaters near the camera. A skater that is not
simulated with the player's physics (a cheap route follower) fills `AudioState` itself; `AudioState::rolling(...)`
gives a documented minimal fill (speed, 4 wheels down, material, board / COM positions and velocities) for that case.

### 3.6 LOD and limits (all retail, applied by the hosts)
| pool | instances | rule |
|---|---|---|
| Traffic (MixMap slot 4) | 4 | nearest 4 by horizontal distance within the 40 m vehicle-audio list (§7.3 G1: 311 / 311; the manager itself not read). The census culls at 110 m, so the engine may stop publishing beyond 110 m. |
| Pedestrian (slot 5) | 15 | the first 15 of the nearest-first ped list cut at 50 m (§7.3 G2, manager `sub_824F2890`); footsteps for the first 3 |
| Player slot instance 1 | 1 NPC skater | the first in `list_order` within **30 m** of the camera while free; held until ≥ 30 m (`skaters::Slots`, retail) |
| Speech | living-world channel not exclusive (the recomp overlaps lines); the queue is 16 requests / 8 streams | Phase 3 |

`WorldAudioInstance` on the holders lets an engine system skip per-frame audio work (foot material lookups, the
NPC `AudioState`) for non-holders. For NPC skaters the engine fills `state` for every skater within 35 m, so that
a claim sees a state in the same frame (the claim happens in the same pass).

### 3.7 Map changes and ownership
- The engine despawns its objects on map change; the bridge sees them vanish and the hosts release them.
- Independently, the hosts reset on `Native::map_epoch` (§2.1), so stale state never survives a bank unload, even if
  an entity survives (e.g. a persistent test publisher).
- `LivingWorldAudio.expected` should be re-set by the system for the new map; the epoch clears the prefetch.
- Mods (§3.9): a mod's objects are despawned when the mod is disabled or reloaded.

### 3.8 Ordering
`world_bridge` runs before `native::mixmap_frame`: it converts components into `WorldOwners` / `NpcSkaters`. The
hosts keep running after `mixmap_frame` until the follow-up moves process into the pre-tick part and update into
the post-tick part of `mixmap_frame`. That is retail's split; today the inputs are one console frame old (33 ms).
Do that move only with a headless before / after render showing the expected one-frame shift and nothing else.

### 3.9 Lua SDK (mods publishing objects): superseded by §8
The user's requirement (2026-10-03): the audio engine is moddable together with the rest of the engine, so
every feature is built with that in mind. §8 is the full mod-facing surface. The sketch below is only its object
part. Mods are first-class publishers (P1, not P4). The sketch is modeled on the fork's vehicle SDK conventions
(keys owned per mod, limits, removal on disable).
New `Command` kinds in `skate-mods` (JSON-validated like `audio_*`):

```lua
sdk.world_audio.spawn('car1', 'traffic', {engine='c04_taxi01', position={x,y,z}, heading=h})
sdk.world_audio.update('car1', {position=…, velocity=…, speed=12, load=0.2, horn=0, skidding=false})
sdk.world_audio.spawn('ped1', 'ped', {voice=59, shoe_class=3, weight=2})
sdk.world_audio.update('ped1', {position=…, feet={true,false}, speech=11})
sdk.world_audio.remove('car1')
```

The host spawns an entity per key with the components above (transform from the updates). Limits: 16 per mod,
64 in all; an update older than 0.5 s parks the object (speed 0). `skater` is **not** offered to Lua at first: a full
`AudioState` from Lua is impractical. Phase 4, after the Rust API settles.

### 3.10 Dev test publisher (audible now)
**It is a Lua mod** (§8.5, `mods/world-audio-test/`, dev only), so it proves the mod surface and the engine
surface at once: the mod commands become the same components. One part stays in Rust: the **ghost skater**,
because a full `AudioState` cannot come from Lua. The mod asks for it with
`sdk.world_audio.spawn('ghost', 'skater', {source='record'|'state_log:<name>'})`, and the host records or replays
the local player's `AudioState`. The behavior below is the same whichever side runs it.

- **Cars:** four lanes around the spawn locator (or the player position at enable). Each is a closed loop of
  straights at 8–15 m/s with accelerate / brake phases (`load` ±, skid on hard braking), models cycling through
  `c00_heavy01 … c08_family03`. One car honks every 10–15 s (kind 1–5), and one parked car fires `VehicleAlarm` on a
  debug key. Sixteen cars in all, so the nearest-4 pool and its hand-over are exercised.
- **Peds:** 20 peds on back-and-forth lines, walking at 1.3 m/s (some jogging at 4 and one running at 8), so all
  three `sk8_foley` ids play. Foot plants come from a gait clock (dev only, not retail), with shoe classes 2–5 and
  voices cycled over the 40 voices that have 501 lines. Speech: a warn (11) when the player passes within 1.5 m
  above 3 m/s, trick nearby (23) on the local player's landed trick within 10 m, and slam (25) on the player's bail.
  It uses the existing `Cues`.
- **Skater:** a **ghost**: on a debug key, record the local player's `AudioState` and transform for 20 s (a
  `VecDeque`, about 1,200 frames), then replay it as an NPC skater offset 5 m to the side. It loops, and walks out
  past 30 m and back, so the claim / release shows. That gives full-fidelity board sounds with no AI. Alternative,
  when the map has `routes`: a rolling-only skater on the first route.
- Gizmos (Bevy gizmos): a box per car, a capsule per ped, a ring for the skater, colored by `WorldAudioInstance`
  (holder or not), plus a 30 m circle around the camera.
- Logs: `AUDIO_WORLD` / `AUDIO_NPC` lines, as today, plus `world_bridge` counts once a second.

### 3.11 Verifying against the recomp (no new user sessions needed for 1–3)
1. **Harness** (headless, existing `tests/world_sources.rs` style, real banks):
   - the map-change regression (§2.1);
   - bridge unit tests in a minimal Bevy `App`: components → `WorldOwners` / `NpcSkaters`, despawn → release, and
     events → state for the right time;
   - the ghost skater's replay renders the same board voices as the local player's recording (instance 1 against
     instance 0, minus the local-only parts).
2. **Level distributions against the recomp's GAIN × SEND**, rendered with the dev publisher's motion: per bank /
   sample group, p50 / p90 against session 164620 (`fstep_livingworld` 0.000 / 0.024, C01 0.002 / 0.049 / 0.281
   max, soft grain members around NPC bursts as in `world-npc-skater-audio.md` §2). Retail mixes over all distances,
   so compare distance-binned once a listener position exists in a trace.
3. **Trace-replay publisher** (headless test + research tool): drive vehicles / peds / the NPC skater from an
   existing session's VEHSTATE / PEDXYZ / SKATEB lines (positions; speeds from position differences; landings).
   The listener is the session's own **`WPPOS`** (the listener in world coordinates, 2 Hz; checked: 164620 at
   100.2 s puts WPPOS at (−199, 4, 639) among PEDXYZ / VEHSTATE / SKATEB points 20–60 m away), or `GRECX` (the
   listener every frame) in the audiox sessions. Compare starts per bank and sample group, GAIN × SEND against
   distance, and the instance-1 contact bursts (`instance1_voices.py`) with the session's own audio lines.
   **Section 7 maps every source to the session that validates it.**
4. **User listening** in game with the dev publisher (the user's verdicts quoted verbatim in doc 11 / the #32
   description), then regression-check (all maps) before calling it ready.

## 4. Phases (all in PR #32, after the code-fix pass)
- **P0: map-change fix.** The epoch reset in `WorldHost` / `NpcHost`, `saturating_sub` on ticks, and the regression
  test. Small and isolated.
- **P1: engine-facing API.** `world_audio.rs` (components, events, resources), `game_audio/world_bridge.rs`,
  `WorldAudioInstance` read-back, `SkaterAudioMemory` + `skater_audio_state` refactor (behavior-identical proof),
  `AudioState::rolling`. Doc: "Hooking up world audio" (docs/hails-additions, doc 11 subsection, plus module docs)
  with the per-field table: retail offset, meaning, who should fill it, default.
- **P2: dev test publisher** (§3.10) and gizmos. The user listens.
- **P3: complete the sources** (research and port, each separately verified):
  1. speech playback in the host (manager + library + slots wired; `speaker_bits`; `GateInputs` from `Cues`);
     the level / pan mapping (research: the SpeechBank stream consumer); the queue / stream limits; values 49 and 29;
  2. NPC instance Wheels / Tricks / Treatment / footsteps / Clothing / board slide; the NPC bail grunt and
     SkaterSpeech;
  3. PedBodyFall (decode), Tazer (AEMS op 38);
  4. the TrafficCarPhysics.in0 writer, the 3DObjPos 4.1–4.3 binding, vehicle `+112` / `+144`;
  5. the retail instance managers (traffic / ped) replacing nearest-N; the ped `+68` footsteps-on rule and the
     `+148` / `+156` pair; the model → engine record / shoe class / weight / voice tables from `livingworld_models`
     (setup export, so the engine names a model and gets the rest);
  6. process / update into `mixmap_frame` (§3.8).
- **P4: the wider mod surface** (§8.2–§8.4): the content layer (bank / sample / program / speech overrides),
  mod emitters on the native voice graph, the tuning read / write, and audio event hooks. `sdk.world_audio.*`
  (§8.1) moves into P1 / P2, because the dev publisher is a mod.
- **P5: verification + docs** (§3.11, §7), the doc 11 sections, PULL-REQUESTS row, and the #32 description
  checklist.

Each P3 item is checked against the existing recordings (§7) as it lands, not in a batch at the end. The two
scripted runs in §7.3 are optional and only fill the listed gaps.

## 5. Things the API deliberately leaves to the engine
Honk and alarm decisions, skid detection, foot plants (animation), speech values (state graphs), the AI skater's
physics, census / culling. The audio side never invents behavior (retail parity): where retail's rule is not traced
(the footsteps-on gate, the near / far pair, the instance managers) the bridge uses a **flagged provisional rule**,
documented as such, until it is traced.

## 6. Open questions (research)
- What retail posts when the skater hits a car (a collision material class for vehicles?).
- `sub_824D8E60`; the ped owner `[obj+28]+144`; the horn kinds (traffic AI); the 3DObjPos 4.1–4.3 binding. (`+148` /
  `+156`, `+68`, vehicle `+112` / `+144` and the TrafficCarPhysics writer: settled, §7.3.)
- The speech stream gain / pan; the living-world queue (why lines overlap).
- What the inactive Player instance's inputs hold after a release.
- Whether remote **multiplayer** skaters count as non-local skaters for the Player slot's second instance, and with
  or without the 30 m gate. `sub_824F1FB0` gates only NPCs by distance, so remote humans might take it without the
  gate. Our remote state carries body / pose only, not a full `AudioState`.

## 7. Data plan: the existing recordings first (user, 2026-10-03)

The user: the play sessions provided so far should hold most of the data needed to get most of it
done. So the build-out and its checks use the recordings we already have. A short **scripted background**
recomp run is proposed only for the gaps in §7.3. A user session is never needed.
Before any use, run `py -3.13 tools/recomp-trace/trace.py <trace>`: **0 malformed lines**, or the
session is deleted (project rule "bad data is unusable"). Every number is "the recomp", not "retail" (the recomp runs uncapped
and stalls; the logic is the strong evidence and timings are weaker).

### 7.1 What is on disk (local recomp sessions, counted 2026-10-03)
Times are the trace's ms. "Listener" = the camera position: `WPPOS` (2 Hz, world coordinates; checked: 164620 at
100.2 s puts it at (−199, 4, 639), with PEDXYZ / VEHSTATE / SKATEB points 20–60 m away) or `GRECX` (every frame).

| session | length | living-world content | audio | listener | NPC-owner caveat |
|---|---|---|---|---|---|
| `all_20261002_163809` | 261 s | PEDXYZ 7,952, PEDSEE 3,320, MOODOUT 142, SECCHASE 1,486, SECTAKE 360, SECTAZE 18, VEHSTATE 2,693, SKATEB 3,455, NPC/VEH spawn + cull | PLAY 23 k, GAIN 36 k (**dsp on**: voice levels), SPLC, POST, READ 436 (speech) | WPPOS | no GREC |
| `all_20261002_164620` | 496 s | PEDXYZ 18,194, PEDSEE 9,016, MOODOUT 631, PEDTIMER 603, SECTAZE 9, **VEHSTATE 10,018**, TRAFENGINE 4,480 (first-pass), SKATEB 7,361, SKATER 267 | PLAY 43 k, GAIN 90 k, SEND 78 k, READ 726; report.md: Traffic_Horn 115, Traffic_Skid 139, car_alarms 12, fstep_livingworld 4,949 starts | WPPOS | no GREC; the reference session (single writer, 0 malformed) |
| `all_20261002_180430` | 676 s | PEDXYZ 7,706, VEHSTATE 1,742, SKATEB 9,040, MOODOUT 102 | PLAY 40 k, GAIN 97 k, CONTACT 28,626 | WPPOS | **GREC 460,476, of which the NPC owner = 227,295** (the NPC's grain bed per frame) |
| `all_20261002_223306` / `223613` | 124 s / 488 s | PEDXYZ 3,963 / 1,813, SKATEB 1,510 / 1,872 | GAIN 10 k / 48 k | WPPOS | GREC 57 k / 352 k; 223613 also TREAT 352 k, SEAMPAT 139 k, SEAMHIT 4 k, with NPC rows |
| `all_20261002_214002`, `214346`, `222155` | 56–104 s | PEDXYZ 1–3 k, VEHSTATE 655 (214002), MOODOUT 9–115 | GAIN 4–8 k | WPPOS | GREC 14–35 k, with NPC rows |
| `all_20261002_204336`, `232153` | 58 s / 234 s | small | small | WPPOS | **clean** (no NPC owner) |
| `audiox_grind_20261003_120315`, `audiox_ride_20261003_141046` | 493 s / 275 s | n/a | GAIN 53 k / 23 k, CONTACT, **COLLPOST 2,398 / 554** | **GRECX** | after the local72 fix: GREC / GRECX are local only. COLLPOST `local72 = 0` rows are the **NPC skater's contact / collision posts**, with positions. |
| `bailx*`, `bandq`, `localtest*`, `riderun`, `bailrun_ok3` (2026-10-03, scripted, Mega-Park spawn) | 76–140 s | n/a | COLLPOST 186–1,216 | GRECX | `localtest2`: the NPC appears at 21.6 s (LOCALTEST proves the gate) |
| `audiox_20261003_095054`, `audiox_bail_094336`, `bailrun_*` before 10:30 | 52–193 s | n/a | GREC / GRECX | GRECX (both owners) | pre-fix: NPC rows mixed in |

The 2026-10-02 sessions that older notes cite (`all_…153038`, `…160126`, `…161849`, `npc_*`) are no longer on disk.
Their extracts remain locally (`ped_chase_*.txt`, `speech_*.txt`, `veh_*.txt`). Our own state logs
(`state_*.tsv`, 27 real-play logs) are the input for the ghost-skater tests; `e2e.rs` already
replays them headless.

**The local72 caveat, turned into data:** before the 10:30 fix, GREC / GRECX / FIRSTHIT / SKID / TREAT / SEAMPAT /
SEAMHIT also logged the NPC skater's objects (15 of 18 sessions; `owner_scan.py` (local script)). Filtering
**by owner** gives the NPC instance's own per-frame records: the inverse of `filter_local.py`, which keeps the first
object each trace logs, so it needs a `--npc` switch. That is retail's instance-1 data for the grain bed, the skid
object, Treatment and Class_Seams, which no new run would give more cheaply.

### 7.2 Source → validating session
| source | session(s) | what it validates (counts / levels / timings / rules) |
|---|---|---|
| Engine bank choice by patch | 164620 (+ 163809, 180430) | every TRAFFIC_CAR start sounds in one `C0n` bank; the share of 7 / 8 against 1 and of 6 against 3 (the `rand()%100` overrides, per vehicle spawn; VEHSPAWN ids join the posts) |
| Engine RPM / Doppler / levels | 164620 VEHSTATE (position + forward, ≤ 4/s → speed) + WPPOS + PLAY / GAIN / SEND for the `C0n` voices | GAIN × SEND against distance (the ~45 m front-layer cut-off, the B7 4–90 m layer); pitch against speed (gear steps at 7 / 14 / 21 / 28 m/s) and the approach / recede shift. Trace-replay: publish the VEHSTATE path headless and compare the per-voice level curves. |
| Front / rear split, heading `+112` | same | the w1 / w2 level ratio against dot(forward, pos − listener): if VEHSTATE's forward explains it, `+112` = heading |
| 3DObjPos 4.1–4.3 binding | same | per-layer level against distance to the body / front / rear points (whichever fits best) |
| Horns / alarms / skids | 164620 (115 / 12 / 139 starts) + VEHSTATE (`+3716` alarm, maneuver `+4396`, target speed) | alarm: starts when `+3716` arms and stops 8 s later, variants spread over 4; horn: variants spread over 9, none from heavy01 (patch 0); skid starts against VEHSTATE decelerations |
| Instance assignment (4 / 15) | 164620 / 163809: concurrent `C0n` voices and footstep owners against WPPOS distances | at most 4 engines at once, and whether they are the nearest 4. This settles nearest-N or shows that another rule is needed. |
| Ped footsteps | 164620 (4,949 `fstep_livingworld` + `sk8_foley` starts) + PEDXYZ (→ speed) + WPPOS | sk8_foley ids 62 / 63 / 64 against ped speed (2.5 / 7.5 m/s); steps per second against speed; GAIN × SEND against distance (p50 0.000 / p90 0.024); the sample groups per class (2, 4, 5 seen); the 151 unexplained starts against MOODOUT times (the jump / collision words) |
| Ped speech: reactions, lines, takes | 163809, 164620 (READ + MOODOUT) + the `speech_*.txt` extracts | event per reaction, start 0.03 s after MOODOUT; the take history (57 / 57 already); `_n` / `_f` against the speaker's distance. The MOODOUT ped address = the PEDXYZ address, measured against WPPOS, which is the join `speech_nearfar.py` lacked. **`+148` / `+156` is likely a distance and its threshold**, fit from the data. |
| Speech level / pan | **163809** (dsp on: GAIN 36 k; the speech voice GAIN 0 → 1, PITCH 0.52–0.70) + the speaker's PEDXYZ + WPPOS | the speech voice's GAIN against speaker distance → which PedestrianSpeech output it follows (B2 out2 / B20 2–70 m) |
| Speech overlap / queue | every session with READ (`overlap.py`) | concurrent living-world lines and the per-speaker spacing (the timers) |
| Tazer | 163809 (SECTAZE 18), 164620 (SECTAZE 9; 49 Tazer starts) | c_tazer / c_tazer_grn_play start / stop against SECTAZE draw / zap: the check for the op-38 port |
| PedBodyFall | 163809 (SECTAKE 360: takedowns knock peds down) | the POST / PLAY lines at takedown times → which class / bank PedBodyFall posts; decode the object against them |
| NPC skater: who gets instance 1 | 164620, 180430 (SKATEB per AI board + WPPOS; the NPC GREC owner appears / disappears) | the 30 m gate: NPC GREC rows start when the first skater in list order comes within 30 m and stop at ≥ 30 m |
| NPC grain bed | **180430** (227 k NPC GREC rows), 223613, 223306, 214002 / 214346 / 222155 | the NPC instance's per-frame bed records (levels, members, soft against the local rider's hard wheels) against our `npc_grains`, driven by the matching SKATEB motion |
| NPC contacts / collisions | audiox_grind_120315, audiox_ride_141046, bailx*, bandq, localtest*, riderun (COLLPOST `local72 = 0`, positions, GRECX listener) | materials, tiers and poster per NPC impact; the non-local branches (no pitch override, spread 0) |
| NPC Class_Seams / Treatment / skid | 223613 (TREAT / SEAMPAT / SEAMHIT NPC rows); pre-fix SKID | the Treatment port for instance 1 (missing now) and the seams that run for NPCs |
| NPC wheels / tricks / footsteps / clothing | POST / SPLC lines with instance-1 MixMap keys (`0x40010820` Wheels, …) in 180430 / 164620 | **checked 2026-10-03: POST lines do not name the instance** (class slot, fresh payload pointer, caller chain) → gap G3, **settled 2026-10-03** (PLAYERPOST run `gapg3_20261003_154858`; see §7.3 "G3 result") |
| Ghost NPC skater (headless) | our `state_*.tsv` logs through `e2e.rs` | instance 1 renders instance 0's board voices, minus the local-only parts |
| Model tables (record / class / weight / voice) | `vlt_livingworld_models.json`, `vlt_livingworld_vehicle_characteristics.json`, `fieldxref*.txt` | whether the record key, shoe class and voice can be read as data; if no field has a reading code site → G1 / G2 |

### 7.3 Gaps the recordings could not close → scripted background runs (DONE 2026-10-03: G1, G2 and G3)
The user approved the runs on 2026-10-03. Each gap got one guarded, read-only hook in skate3recomp `research-hooks`
(uncommitted), fixed field counts in `tools/recomp-trace/trace.py` `FIELD_COUNTS`. Runner `recomp_script_run.sh`,
`BACKGROUND=1 MUTE=true SKATE3_TRACE=audio,npc,traffic`, autostart 8000, one game at a time. Sessions (all 0 malformed
lines, generic check and fixed-layout check): `gapproof_20261003_152333` (Crystal Towers 30 s: both hooks fire),
`gapg1_20261003_152507` (Crystal Towers: stand 60 s, forward 6 s, stand 30 s), `gapg2_20261003_152734` (Aletown:
stand 60 s, walk 10 s, turn, walk 10 s, stand 20 s). Scripts `gap_{proof,g1_traffic,g2_peds}.txt`.
Analysis: the local tool `world_gaps.py <trace> [--veh|--ped]`; outputs saved as
`gaps_g1_152507.txt`, `gaps_g2_152734.txt`, `gaps_g2_from_g1_152507.txt`. Most answers came from
reading the code and the attribute database first; the runs confirm them. All numbers are "the recomp".

Hook line formats:
- `VEHAUD` (traffic, 15 fields): TrafficEngine update `sub_824D6478`, read after it runs. obj | R = `[obj+28]` | entry id
  `[[R+40]+64]` | key R+168 (hex) | patch obj+48 | rpm obj+52 | pos R+48 | fwd R+112 | R+144 | R+148 | R+152 | horn
  R+156 | skid R+160 | R+176 | listener `*(0x830CFDD4)`+0. Per object ≤ 4/s, at once when horn / skid / key / patch change.
- `PEDAUD` (npc, 21 fields): PedestrianSFX process `sub_824D8078`, active instances, read after it runs. obj | S =
  `[obj+32]` | id S+64 | list index | list count | bytes "+68 +69 +71 +80" | feet "+73 +74" | S+84 | record key of S+84
  | S+96 | "S+88 S+92 S+120 S+124" | S+132 | S+136 | "S+140 S+144" | S+148 | S+152 | S+156 | O+128 (O = `[obj+28]`) |
  O+144 | O+48 | listener. Per object ≤ 2/s, at once when id / +68 / +136 change.

#### G1 result: the vehicle audio record (settled)
- **Writer:** `sub_824B2A28` fills the record R each frame from a 32-byte **vehicle-audio entry** in the array at
  `[G+0x2F0A0]` (G = `*(0x83083C38)`), matched by entry +12 == `[[R+40]+64]`: entry +0 → R+144, +4 → R+148 (speed),
  +8 → R+152, `+16 >> 28` (signed) → R+156 horn state, bit 27 of +16 → R+160 skid, +24 (u64) → R+168 record key.
  When no entry matches, the record is not touched (see "stale" below).
- **R+112 = the heading:** row 2 (forward) of the vehicle's world matrix (the same matrix VEHSTATE reads), normalized.
  Trace: |R+112| = 1.0000 ± 0.0001; R+112 · VEHSTATE forward p05 0.9986 / p50 1.0000 (328 joins); R+112 · direction of
  travel (moving > 2 m/s) p50 1.000 (445 pairs). So `TrafficAudio` takes the heading from the transform (no velocity
  override needed).
- **R+128 = heading × speed** (velocity along the heading). **R+80 = pos + heading × 1 m, R+96 = pos − heading × 1 m**
  (constants +1.0 / −1.0 at `0x8231A844` / `0x8216DEE0`): front and rear points 1 m from the body, the likely targets of
  3DObjPos 4.1 / 4.2 (binding still to confirm; was "all at the body").
- **R+144 = the driver's signed acceleration (m/s²)**, not a throttle: −15.64 through a hard stop, then 0.4, 1.0, 1.4,
  2.0, 2.4, 3.0, 3.02 while pulling away; the speed changes by exactly R+144 × 0.2 per traffic step (15.6 → 0 in steps of
  3.128 = 15.64 × 0.2; 3.004 → 6.93 in steps of 0.604 = 3.02 × 0.2). Range in the run −15.64 … 3.02; 0 while cruising.
  So `TrafficAudio.load` = acceleration in m/s² (the engine / skid words use it × 3000), and the bridge can derive it
  from the speed change when the engine does not give it.
- **R+152 = horizontal (x/z) distance to the listener** (matches to ~0.02 m on live records). **The list is cut at
  40 m**: R+152 never exceeds 39.994 (1,175 lines).
- **R+176 + the `TrafficCarPhysics.in0` writer (static, `sub_824B2A28` tail):** |R+128 − `[listener+48]`| (the
  relative speed; listener +48 is taken to be its velocity), clamped to 35 (`relative_velocity` record, class
  `0xC1831BDB6CB1B1EA`, field `11FAA9AADDC78EC0` = 35), slewed by 100 × dt (field `AB85397C101B0752` = 100), stored at
  R+176, then `R+176 / 35 × 32767` → `[R+12]` vfunc +8 input 0. The same formula as the NPC skater's PlayerPhysics in13.
  In the run R+176 tracks the speed (listener standing; max 24.6). This opens the A11 near boost gate: port it.
- **Model → engine record (static, attribute database; confirmed):** `livingworld_entities` field `92A043B4A11F1A2A` →
  `livingworld_vehicle_characteristics` record → field `BA2DDD830C731EE4` → `aud_traffic_engine` record:

  | entity (and children) | vehicle spec | engine record (patch) |
  |---|---|---|
  | sedan, sedan01–04, old_sedan, hatchback01 | vehicle_spec_family01 | c01_family01 (1 → 7 / 8 by `rand()%100`) |
  | sports, sports01–03, muscle01, z_pipeline_lw_vih | vehicle_spec_sports01 | c03_sports01 (3 → 6 at 50 %) |
  | taxi, taxi01, patrol01 | vehicle_spec_taxi01 | c04_taxi01 (4) |
  | suv, suv01–02, pickup01–02 | vehicle_spec_truck01 | c05_truck01 (5) |
  | minivan01 | vehicle_spec_minivan01 | c05_truck01 (5) |
  | vehicles / pickup / hatchback / muscle / minivan (abstract parents) | default | default (patch 2: silent) |

  `c00_heavy01` (patch 0) and `c02` are never referenced, so "heavy01 never honks" does not arise in free roam.
  `c06_sports02` / `c07_family02` / `c08_family03` are reached only through the patch override (their records also
  exist; c08's own patch field is 7). Trace: the record keys are only c01 / c03 / c04 / c05; patches chosen per vehicle
  c01 {1: 6, 7: 3, 8: 4}, c03 {3: 1, 6: 1}, c04 {4: 4}, c05 {5: 8} (27 vehicles).
- **Instance assignment:** every engine instance holder was among the 4 nearest vehicles by horizontal distance (311 of
  311 holder-seconds, VEHSTATE positions), consistent with nearest-4 within the 40 m list. Keep nearest-4 (no longer
  provisional for the choice; the manager itself is still not read).
- **Stale records:** when a held vehicle leaves the 40 m list, its record keeps its last values (position, speed 10.65,
  R+144 3.02 …) while the instance stays held, so retail keeps sounding the frozen car until the instance is reassigned.
  Recorded as behavior, not ported unless a listening check asks for it.
- **Horns:** one horn in the run: kind 1 together with skid 1, at the start of a c01 car's hard stop (−15.64 m/s²) 18 m
  from the listener. **Horn kinds per model: still open** (only kind 1 seen; the kind is packed by the traffic AI into the
  top nibble of entry +16, writer not found). They are the AI's choice, so the API keeps `HornState::Honk(1..=5)` as an
  engine input.

#### G2 result: the ped audio state (settled)
- **Instance manager `sub_824F2890`** (vtable slot at `0x822FD618`, called once per frame): reads the packed 20-byte **ped
  audio entries** at `[G+0x2F070+56]` (count `[G+0x2F070+60]`; zero in game modes 18–20 or when `[*(0x830CFDC4)+560]` ≠
  1), takes the first min(count, 15), and fills each held instance's state S (instances matched by S+64 == entry +12;
  unmatched instances are deactivated through vfunc 28, new entries claim one through the manager's vfunc 3):
  - S+136 = entry +0 & 0x7F (the speech value); S+73 = bit 29 (foot B), S+74 = bit 14 (foot A), S+71 = bit 13;
  - S+140 = (entry +0 >> 16) & 0x7F, S+144 = (entry +0 >> 23) & 0x7F (materials A / B);
  - S+120 = 1 << (((entry +4 >> 25) & 31) − 1) (0 when ≥ 18); S+80 = bit 17 of +4; S+92 = field `02D9BFDA94A8B35E` of the
    `aud_characteristics` record of model (entry +4 >> 18) & 0x7F (the partner's / target's type: varies per line);
  - S+148 = entry +8, S+76 = entry +16;
  - **S+68 (footsteps on) = (the ped's list index < 3)**, 3 = `aud_speech/default` field `53364DFA09A499DD`;
  - S+69 = (entry +8 < 50), 50 = `aud_speech/default` field `7F0FFDF7A90577C6`.
- **The list is sorted nearest first and cut at 50 m.** S+148 = the 3-D distance from the ped (O+48) to the listener:
  corr 1.0000, median |diff| 0.047 m (G2 run, 1,130 lines); list order agrees with S+148 in 1,935 of 1,936 same-moment
  pairs; S+148 never reaches 50 (max 49.9995 over 3,146 lines in three sessions). So the 15 instances = the 15 nearest
  peds within 50 m (nearest-15 confirmed), and **footsteps play for the 3 nearest only**: +68 == (index < 3) in 2,803 of
  2,803 lines; 3 instances had +68 = 1 in 369 of 388 half-second bins (the rest are hand-over moments).
- **+148 / +156, the near / far pair:** +148 = distance to the listener (above); **+156 = the model's
  `aud_characteristics` field `A27215A909135B62`** = 20 m for every `regular` ped (5 in `default`, 30 for pros / IP /
  marquee). PedestrianSpeech's flag 1 (= the `_f` / far lines) ⇔ distance > 20 m. Trace: S+156 = 20.0 in all 2,803 lines.
- **Per-model fields (activation `sub_824F91B0`, from the `aud_characteristics` record of model S+84; the per-model record
  keys live in a table at `*(0x830CFDDC) + (model + 14287) × 8`, default record `default`):**
  - **S+84 = the model = the speech voice id** (the record's `Character` field: 41 adult_male_1 … 96 ai_skater_female_3);
  - S+132 shoe class = field `871BDC669F2B1844` (default 2); S+96 kind = the speech type bit, field `492964E71634DA6D`
    (so "kind 64" = the three `security_guard_*` records, the close-range level output); S+124 = field
    `68BB61E508841729` (1 female, 2 male); S+156 = field `A27215A909135B62`; S+88 = the voice variant bit
    (1 << (n − 1) for the `_n` record: business_male_1/2/3 → 1/2/4, adult_female_4 → 8); S+152 = a per-voice float
    (`sub_824A9E48`(voice), 0.80–1.15, e.g. 53 → 0.80, 94 → 1.15; probably the speech pitch, unconfirmed).
  - Trace: 20 models seen over both runs, every S+96 and S+132 equals the database (e.g. 41 → 1 / 5, 59–61 → 4096 / 5, 66 → 8192 / 4,
    52 → 4 / 4, 73 → 16 / 2, 87 → 16384 / 2, 91 → 512 / 2).
  - Shoe classes in the database: 2 for most; 3 security_guard_1–3 and adult_male_3; 4 business_female_2/3,
    adult_female_3, granny_2; 5 business_male_1–3, adult_male_1, tourist_male_1 (all 112 records checked, with
    inheritance). **No model uses class 1 (silent).**
  - Full table: the local tool `vlt_show.py aud_characteristics` (written by
    `vlt_resolve.py '^aud_characteristics$'`).
- **O+144 is not a per-model weight:** it is 1 for every ped beyond ~12 m and 2–5 for some peds close to the listener
  (model 73 within 12 m: 2 / 3 / 4 / 5), so it is dynamic per ped (meaning open: a gait / animation LOD state?). O+128 is a
  noisy per-frame speed (corr 0.38–0.41 with position differences; p50 0.3–0.8 m/s for walking peds).
- **Materials:** S+140 / S+144 were 0 in every line (Aletown and Crystal Towers pavements), never 143.
- Speech values seen: 3 (most), 1, 2, 4, 33, 36, 38–41, 48, 56, 63.

#### G3 result: what the NPC skater's Player instance plays (settled 2026-10-03)
The older `POST` lines do not name the instance (class slot, fresh payload pointer, caller chain; the posters are the
same code for both Player instances), so G3 got a new hook (user-approved 2026-10-03).

**Hook `PLAYERPOST`** (category `audiox`, skate3recomp `research-hooks` `hooks_audio.cpp`, uncommitted; 12 fields in
`trace.py` `FIELD_COUNTS`): the per-player SFX objects' vtable process / update functions are wrapped (SkateBoard
`0x822FC770` +36 / +40, Contacts `0x822FC698` +32..+40, Tricks `0x822FC7B8`, Wheels `0x822FC848`, Clothing
`0x822FCD10`, Treatments `0x822FCDA0`, OffBoard `0x822FCF98`), so the object being run is known on the thread; every
`POST` (`sub_828E2B48`) and `SPLC` (`sub_82975700`) made inside one is logged with that object, plus the Wheels stream
start / stop (`sub_824CEAF0` / `sub_824CEF60`) and the NPC bail grunt helper `sub_824BF5F8`. Fields: class | via (POST /
SPLC / WSTART / WSTOP / BAILGRUNT) | object | local72 `[[obj+28]+72]` | local16 `[[obj+16]+72]` | MixMap key
`[[obj+12]+4]` | record id `+64` | record skater index `+68` | a (slot / bank / layer) | b (payload / id / started) | words
| chain.

**Runs** (background, muted, `SKATE3_TRACE=audio,audiox`, autostart 8000; one game at a time): `g3proof_20261003_154754`
(Mega-Park spawn, 4 pushes, stand; 270 lines, both instances fire) and **`gapg3_20261003_154858`** (script
`gap_g3_ride.txt` = `ride_spawn.txt` + 60 s standing at the marker + segment 1 again; 160 s; 851
lines). Both 0 malformed (fixed-layout and generic checks); the audio captures were deleted. Analysis
the local tool `player_posts.py <trace>` → `gaps_g3_154858.txt`
(`gaps_g3_proof_154754.txt`). All numbers are "the recomp".

**Instances and keys:** instance 1's objects carry the instance-1 keys, base `0x40010800` + the class offset:
SkateBoard `0x40010800`, Contacts `…810`, Wheels `…820`, Clothing `…860`, OffBoard `…890` (instance 0: `0x400100xx`,
also Tricks `…050`, Treatments `…070`). local72 = 1 on every instance-0 line and 0 on every instance-1 line (851 / 851).
Instance 1 was claimed 12 times in 150 s (each claim re-creates the OffBoard's two footstep packets) by three skaters
(record ids 1, 2, 3) in turn, ~74 s held in all (a lower bound: window ends are the last line or the stop-all at the
deactivation); handovers between skaters are frequent at the park (one 0.1 s and one 0.5 s hold).

**Per component, local (per run second, 150 s) vs NPC (per held second, ~74 s):**

| component | local rider (instance 0) | NPC skater (instance 1) | port consequence |
|---|---|---|---|
| SkateBoard posts | 74 (0.49/s): wheel skid (slot 1) 44, rattle (10) 20, Class_rolling held layers (42) 10 | 67 (0.90/s): wheel skid 42, squeaks / powerslide (7) 14, rattle 11; **no slot 42** | as ported (one rolling pass, no held layers 0 / 3); skid / squeaks / rattle run per instance |
| Contacts Splice | 243 (1.6/s): sk8_foley 94 / 95 / 92 / 93, Skate_Collisions 1112 / 1115 / 1095 / 1060 … | 264 (3.6/s): the same families (sk8_foley 95 / 94 / 92 / 93, Skate_Collisions 1112 / 1115 / 1111 / 1079 / 1091 / 1124 / 1126 …) | as ported (non-local branches in `player::contacts`) |
| Contacts foot drag (slot 0) | 1 | 0 | n/a |
| **Bail grunt** (`sub_824BF5F8`) | called once, does not post (local72 = 1) | **called once and posts** the speech message (8206 / 115) at an NPC bail (140.3 s, with the NPC's Clothing body slide and cloth falls posts) | reachable: port in the speech stage (PlayerSpeech record of that skater) |
| **Wheels** spin streams (`SFXObj_Wheels`) | 14 starts, all layer 0 (state `+332` = in the air), level 4.2–13.7 | **18 starts: layer 0 × 17, layer 1 (`+340`) × 1**, level 0–12.9 (p50 6.3); each stopped (layer off), plus a stop-all at each release | **runs for the NPC**: port Wheels for instance 1 (key `0x40010820`); layer 2 (on foot / `+308` / `+760`) and the extra parameter writes after a start are gated by `[[obj+16]+72]` = local only |
| **Tricks** (Class_Flips slot 4, cloth_trick slot 22) | 12 posts | **0** | **local only**: the Tricks process `sub_824CBEB0` returns at once when local72 = 0 (static) → no NPC trick sounds |
| **Treatment** (slot 9) | 1 | **0** | **local only**: the Treatments process `sub_824DD408` returns at once when local72 = 0 (static) |
| **OffBoard** | 2 footstep-packet creations; 38 Splice starts (Skate_Collisions 963, sk8_foley 86 / 105 / 107) | 24 packet creations (2 per claim), **0 Splice starts** | the packets exist per instance, but the per-foot step sounds require local72 (spec `aems-offboard-clothing-spec.md` §2.1) → no NPC footsteps. The walking / jump voices (`sub_824E9D10` / `sub_824E9678`) are not gated (local72 only selects one start-block float) but the NPC was never on foot in the run: unobserved |
| **Clothing** | 8 posts (body slide 21 × 6, cloth falls 23 × 2); 28 Splice (sk8_foley 74 × 20, 73 × 8) | 3 posts (body slide × 2, cloth falls × 1, at the bail); **20 Splice (74 × 11, 73 × 9)** | **runs for the NPC**: port Clothing for instance 1 (key `0x40010860`); non-local start block: float `+112` = 0.0 instead of 1.0 (the "spread 0" of the Contacts non-local one-shots) and local72 passed to the eq-chain pick `sub_82491108` (create flag) |
| board slide (slot 15) | 0 | 0 | not reached in the run (SkateBoard process `sub_824C6A78` → `sub_824CB3C8`, no local gate statically) |

Still open: the walking / jump voices for an NPC on foot (not seen); board slide for either instance; the Wheels start
words `r5` / `r6` are two alternating stream records (pointers), not ids; the NPC bail grunt's speech playback (speech
stage).

Not gaps: reactions, chases and speech events (MOODOUT, READ, the extracts); traffic driving and culling (VEHSTATE); the
NPC skater's board data (GREC NPC rows, COLLPOST).

## 8. Mod-facing surface (user, 2026-10-03: "make the audio engine moddable with the rest of the engine")

The engine-facing surface (§3) is ECS components. The mod-facing surface is **API-2 commands** (`skate-mods`
`Command`, JSON-validated like `audio_*`, Lua wrappers in `sdk/skate.lua`). The modding bridge
(`skate-game/src/modding/`) turns those commands into the same components / events. There is one path into the
audio, and mods and engine systems are equal publishers. General rules come from the existing mod rules: keys belong
to the calling mod; per-mod and global limits; validation before allocation; commands from a failed callback are
dropped; disabling or reloading a mod removes everything it published or overrode, and the retail behavior comes
back deterministically. The existing `audio_preload / play / update / stop / stop_all` keep working unchanged.

### 8.1 World objects and their one-shots (Phase 1–2)
```lua
-- spawn / update / remove: kind = 'traffic' | 'ped' | 'skater'
sdk.world_audio.spawn('car1', 'traffic', {engine='c04_taxi01', position={x,y,z}, heading=h})
sdk.world_audio.update('car1', {position=p, velocity=v, speed=12, load=0.2, skidding=false})
sdk.world_audio.event('car1', 'horn', {kind=3, seconds=1.2})   -- 'alarm' (8 s, retail), 'horn'
sdk.world_audio.spawn('ped1', 'ped', {voice=59, shoe_class=3, weight=2, position=p})
sdk.world_audio.update('ped1', {position=p, velocity=v, feet={true,false}, materials={40,40}})
sdk.world_audio.event('ped1', 'speech', {value='warn'})       -- SpeechValue name or number
sdk.world_audio.spawn('npc1', 'skater', {source='lite'})      -- or 'record' / 'state_log:<name>' (ghost)
sdk.world_audio.update('npc1', {position=p, velocity=v, speed=6, wheels={true,true,true,true},
                                material=40, grinding=false, air=false, land=false})
sdk.world_audio.remove('car1')
local info = sdk.world_audio.read('car1')  -- {audible=true, instance=2} (`WorldAudioInstance`)
```
- `skater` `source='lite'`: the host builds an `AudioState` from the few fields a script can give (§3.5
  `AudioState::rolling` plus grind / air / landing flags). Rolling, grind and landing sound right; tricks, foot
  and body foley stay silent. This is documented as such.
- Limits: 16 objects per mod, 64 in all (counted with engine objects for the audible pools only: the retail pools
  decide who sounds).
- An object not updated for 0.5 s is parked (speed 0, feet up), like the fork's vehicle SDK's control timeout.

### 8.2 Content layer: replace or add banks, samples, programs, speech lines (Phase 4)
This is the "Data-driven overrides" goal in the native-port plan ("Modular and moddable"). The world
sources need it too, so it is designed once here:
- **Resolution by retail identity:** `Library::bank_source(stem)` and the sample / program lookups gain an overlay.
  Mods first (in load order), then the install. Identities:
  - a bank stem (`C04_taxi01`, `Traffic_Horn`, `fstep_livingworld`);
  - a bank + sample index (one WAV replaces one sample);
  - a bank + patch program (an `.abk` built by the existing pipeline, or a whole replacement bank);
  - a speech clip + take (`501_59_busm1_Warn_n` take 3), or a new clip for an existing event / voice. The speech
    index and the `.evt` records gain mod rows, so the rules can pick them.
- Declared in `mod.json` (`"audio": {"replace": {...}, "add": {...}}`), validated (paths, PCM16 WAV limits as
  `audio.rs`, sizes against a per-mod budget). A bad entry is reported and skipped; the retail sound stays.
- Mods with overrides of loaded banks force a reload of those banks at enable / disable (the same path as the
  map-change reload; the §2.1 epoch reset makes the world hosts re-post).
- A **new** bank needs a class to post to. Mod banks bind to an existing class (e.g. a new engine sound bound to
  `TRAFFIC_CAR` with its own patch number above 9) or to a mod class name posted via §8.4.

### 8.3 Emitters and tuning (Phase 4)
- `sdk.world_audio.emitter(key, {bank='Siren_city_1', patch=225, position=p, extent=e, volume=v, falloff=f})`:
  an `.ems`-type-1 emitter record added to the map's list, played by `emitters.rs` through `c_emitter` on the native
  voice graph (reverb, buses and ducking included). This replaces the separate Bevy voices for mods that want
  "a sound in the world".
- **Tuning read / write:**
  - `sdk.world_audio.tuning('traffic_engine', 'c04_taxi01')` returns the record, and `…set(…)` overrides fields
    for this session;
  - the same for `ped_footsteps` and the speech `EventTuning` (probability, timers per event).
  - Retail values stay the defaults; overrides are per mod and are dropped on disable.
  - **Not** the retail constants that define instance layouts (4 / 15 / 2 instances, 30 m): those are the MixMap's
    shape. Changing them needs a MixMap rebuild; listed in §8.6.

### 8.4 Posts, globals and event hooks (Phase 4, shared with the player audio)
- `sdk.audio.post(class, words)` → handle; `redeliver(handle, words)`; `release(handle)`. Rate-limited; at most 32
  held handles per mod. This plays any retail sound with real payload words.
- `sdk.audio.global(name)` / `set_global(name, v)`; `sdk.audio.mixmap(slot, instance)` returns the outputs (read
  only).
- Event hooks: `on_audio_event(event)` with `event.kind` = `speech_request` (ped, value, event id, line), `horn`,
  `alarm`, `npc_claim` / `npc_release`, and the player ones (pop, land, grind on / off, zone change). A hook can
  return `suppress` or layer its own post. Callbacks get these as plain data; nothing AEMS-internal is exposed.

### 8.5 Example mod = the dev test publisher
`mods/world-audio-test/` (`mod.json` api 2, `main.lua`; dev only, not shipped upstream unless the user asks). It does
§3.10 in Lua:
- cars on loops around the player's position at enable, honking, skids on braking, an alarm on a menu button;
- peds on lines with a gait clock and speech on proximity / tricks (reads the player state from the existing
  `event` fields);
- a ghost skater (`source='record'`);
- settings: counts, speeds, which kinds;
- UI: a small mod window with toggles and the `read()` audible flags.

`check_mod` validates it. The mod-making guide gets a "World audio" section and the "Planned audio modding" block
is updated. Phase 4 adds a second example, a "louder horns / custom engine" mod using §8.2–§8.3.

### 8.6 What is not moddable yet (input for later passes; read from the code 2026-10-03)
| hard-coded today | where | what a mod cannot do |
|---|---|---|
| Bank lists `TRAFFIC_BANKS`, `PED_BANKS`; the player `components::BANKS` / `OPTIONAL_BANKS` | `skate-audio/src/world/mod.rs`, `game_audio/player_audio.rs:598–605` | add or swap a bank for a source |
| The keep-list of `unload_map_banks` (utility, seams, player banks) | `native.rs:406` | keep a mod bank across maps; mod banks would be unloaded like map banks |
| Bank resolution only from the install manifest (`Library::load` / `bank_source`) | `library.rs:837 / 1027` | override any bank, sample or program (no overlay) |
| Class names as constants (`TRAFFIC_CAR`, `TRAFFIC_HORN`, `c_car_alarm`, `TRAFFIC_SKID`, `livingword_footstep`, the `sk8_foley` step ids 62 / 63 / 64) | `world/traffic.rs:27–30`, `world/peds.rs:27–31` | point a source at another class |
| World tuning (`aud_traffic_engine`, ped footsteps, speech tuning) only from `audio_manifest.json` `world_tuning`, read once (`host.ped_tuning` at the first owner) | `world_sources.rs`, `world_audio.py` | change it at run time |
| The player's vault tuning (`PlayerTuning`, contact tuning, the bridge's speed graph) | `player_audio` / `library.rs:1049` | tweak player sound tuning |
| Speech index / takes from setup only (`speech/livingworld.json`, decoded WAVs); playback not hosted | `world_audio.py`, `world_sources.rs` | add lines, voices or events |
| Emitters only from the map's `.ems` files; the `PROFILES` fallback table | `emitters.rs` | add or move emitters (mods use the separate Bevy voices instead) |
| Location sets, zone ambience, crossfade groups, region layers from the manifest | `random_sets.rs`, `ambience.rs`, `crossfade_groups.rs` | change set data, beds or weights |
| Mod sounds are plain Bevy voices outside the native mixer | `modding/audio.rs` | use the reverb / buses / ducking / MixMap distance curves |
| Instance layout: 4 traffic, 15 peds, 2 player instances, 30 m (MixMap `RETAIL_INSTANCES`) | `world/keys.rs`, `world/skaters.rs:60–63` | more audible cars / peds / NPC skaters (needs a MixMap rebuilt with more instances: a deliberate non-retail option) |
| The `SKATE_AEMS_*` switches are process env vars | throughout `game_audio` | per-mod or per-session toggles |
| No AEMS post / global / MixMap read access and no audio event callbacks in the SDK | `skate-mods` | play retail sounds by name, react to game sounds |
| The world owners' `rand()` generator (`Lcg(0x5EED)`) | `world_sources.rs:216` | seed for reproducible mod tests (minor) |

## 10. Gaps closed (2026-10-03, doc 15 "Gaps closed")
- **Tazer** ported: `SFXObj_Tazer` decoded (`world-ped-audio.md` "Tazer and PedBodyFall ported"); `c_tazer` held
  while `S+80`. vs 164620: 19 starts per 2 s hold (recomp 10 / 20 / 19 = 49), gaps 192 / 192 / 128 / 96 vs
  190 / 190 / 130 / 90–100, first-start gain recomp / ours 0.85–1.00, order 8, 7, shuffles of 0–6.
- **PedBodyFall** ported: the trigger is the animation's `BodyFallType` (`S+76`); the recordings hold 75 starts
  (SPLC, not POST). 73 / 73 containers by type; voice gain recomp / ours p50 0.79.
- **Speech** (`world-speech.md` "Speech details resolved"): PEAK = azimuth head-shadow curves (6 / 6 recomp pairs on
  the curves), Send A = out21 (pre-gain) and Send B = out15 (echo submix, not played), value 49 = phone ring →
  64 (2 / 2 in 164620), value 29 = 1 s repeat with the game flag, Obj:Speech in0 / in1 / in4 by speaker id ranges,
  first-free stream (95 / 109), constructor last = 68, main-cast mapping decoded (port needs the main-cast
  index / decode). Open: the queue clock's unit, the echo submix, in2 / in3, the per-voice float, the near lines'
  +0.6 dB.
- **NPC board slide** ported on static evidence (no local gate; `+780` per skater entry). Scripted background run (Mega-Park, 4 min standing, PLAYERPOST, 0 malformed): 12 instance-1 holds (78 s), no
  NPC bail, so no slot-15 post by either instance: still unobserved.
- **NPC grain bed vs 180430** (filtered by owner): near band A gain 0.097 / 0.108, further bands louder in ours
  (the local-skater distance is not joined); pitch matches.
- **Update (same day): echo submix and main cast ported** (`world-speech.md` "The echo send and the main cast"). The
  pre-gain send (out21) feeds the echo submix, out15 goes to the env bus (corrects the line above); the voice float
  is applied; the queue clock = visual game ticks. Main-cast channel: pro peds, NPC skater reactions / crash
  (`NpcSkaterAudio::reactions`, `NpcSkaterReactionEvent`, mod event `reaction`), the message pairs; decode 6596
  takes / 922 MB; all 82 recorded main-cast lines reachable through the ported words.

## 11. Session marker sounds (2026-10-04, doc 15 "Session marker sounds"; follow-up branch `audio/respawn-marker`)

The front-end sound path, decoded from the recomp (TU3; reference only):

| Step | Code | What |
|---|---|---|
| Ask | `sub_825DFAF0(key64, flag)` | looks up the `fe` record (class lookup8 `5831CB95F3E90598`); if it has a sk8_menu id, a HOM id or a moment SFX, calls the audio system's vfunc `+44` (`sub_82482B10`): queues message `0x34FF4A33` {key, flag} |
| Receive | `sub_824955A8` → `sub_824955B8` | on the audio thread: record `+8` (sk8_menu id, ≥ 1), `+4` (level), HOM `845A10052522A2FB`, moment `A4080FB65880E3C2`, bools `9574DCC8B216CE79` / `BF45D439FAC71A2E` (second bus) |
| Slot | `sub_82495828(this, key, bank 5, id, …, repeat, alt, level)` | level ≤ 0 → nothing; first of 10 slots (32 bytes from `this+32`: key, bank, id, handle, level, repeat, pending, alt) with neither pending nor handle |
| Play | `sub_824958F0(this, dt)` (from `sub_82495528`, the audio update `sub_824854A8`, `this` = audio system + 64) | pending → `sub_82975700(bank, id, out)` + `sub_82975A60` with `[level·v, 1, 0, 0, 1, 1]`; playing → `sub_82975B08` with `[level·v, 1, 0, dt, 1, 1]`; ended → release, re-request when `repeat` |
| Volume | `v = [[X+88]+40] × 1/32767` (`+44` unless bank 5 / 2) | = MixMap Master out 0 (14568 in free skate; watch run `marker_fevol`: `+40` 14568, `+44` 14568, `+48` 32692) |
| Output | `out = [[[0x830CFDBC]+8]+52]` (mastering graph) or `[this+360]` with the alt flag | no env send, no eEQChain |

Session marker posts: `sub_82898FC8` action 42 → `0D6C88A3B91C828F` (`cellphone_place_marker`, placed) /
`66B3AFE3B602918C` (`cellphone_marker_error`, refused); the relocation tick → `7F135F9FD28F7F21`
(`cellphone_goto_marker`); `sub_826682B0` (cellphone UI, state 2, input 4) → `47FE75BF61F19941`
(`cellphone_activate`). Records: 235 / 0.5, 237 / 1.0, 209 / 1.0, 236 / 1.0 (sk8_menu id / level).

Measured (SPLC bank index 5 = sk8_menu, user sessions `audiox_bail_20261003_094336`, `audiox_20261003_095054` and
scripted bail runs; 50 / 11 / 18 events): steady gains activate 58 0.6288 / 59 0.1572; place 58 0.8892 / 40 peak
0.3048 / 59 0.6288; go-to 58 0.6288 / 36 0.4446 / 40 peak 0.2223 / 59 0.6288 = member gain × record gain × level ×
14568 / 32767. Delays after the post 10 / 46; 13 / 77 / 144; 8 / 162 / 189 / 278 ms (members' `+20` delays).
Port: `skate_audio::frontend`, `game_audio::frontend`, `crate::ui_audio`.
**Go To Marker's "static" (2026-10-04, user: the marker sounds sound good, but no static noise plays with the
transition back to a marker as it does in retail).** No separate sound in
retail: the hold sends `cMsgTeleportEffectAmount` (`0xFAF37902`, progress at `+16`, every tick plus 3 ticks of 1 after
the jump) whose only game listener is the VisualDirector (`sub_827A96F8` registers, `sub_827A9C60` stores `+48`,
`sub_827AAF10` draws) = the screen static, visual only; the relocation tick queues `Flow::Teleport` (`sub_825582D0`)
and asks `cellphone_goto_marker`, the only `fe` record of the path (361 `sub_825DFAF0` calls listed). Around 23
recorded go-tos only the record's sk8_menu 58 / 36 / 40 / 59 follow (plus the ride's SenseOfSpeed POST 12 and one
quiet pass-by whoosh, Sk82_Whsh_Bys id 91, `sub_824D2C70`, gains ≤ 0.021); the recomp's capture (20 go-tos) shows above 4 kHz
only 58's and 59's noise bursts (0 … 40 ms and 270 … 300 ms after 58's onset). So the static is samples 58 / 59
(white-noise bursts) of the record, which the port already plays in the same shape. Nothing ported; open with the
user (doc 15 "Follow-up: the static noise on Go To Marker"). Tools: local, not published.

**Far return (2026-10-04, investigating).** The user: (1) on console it lasts about a second, and only after traveling farther away from where
the marker was set; (2) yes, the go-to sound (the sound when you return to the marker) has it; (3) maybe. The hold is 0.2 s up to 100 m, `d / 1125 + 1/9` s to 1000 m, 1 s beyond (the static
ramps over it). The request goes to `sub_82709740` → `sub_82706F50`, which asks `sub_82864C40` whether the destination is
streamed in (radius 30 / 50 m): if not, the loading state (`0x82707508`, event 7) runs a loading screen. The user's
recording `audiox_marker_20261004_123843` (0 malformed): the far return showed the loading screen; the record played,
then the whole mix was cut at ~+420 ms and stayed silent through the load; no other sound (no fe, POST, stream). The
short return (83 m) was the record over the ride. No ~1 s static sound in the recomp (its loads and audio timing differ
from the console). New hooks `hooks_marker.cpp` (audiox: TPMARK, TPDEC, TPSTREAM, TPFX, FEREQ, GSTATE, GEVENT, HUBMSG)
for the next recording; doc 15 "Follow-up 2: the far return". Resolved below (the noise is the hold's Treatments crackle; that recording had it too, below 4 kHz).

**The far-return noise: Class_Treatment's teleport crackle (2026-10-04, ported; doc 15 "Follow-up 3").** User, after
a recording with the marker hooks: "it played the noise! its the first return in the recomp run i just finished". The
noise is not a front-end sound. It comes from the skater's `Class_Treatment` (bank `Treatments`, the packet posted
once and held), which plays slots 1–12 while the teleport effect is on and slot 0 when the effect reaches 1.0.

| Step | Code | What |
|---|---|---|
| Send | `sub_82898FC8` | `cMsgTeleportEffectAmount` (`0xFAF37902`, amount = hold progress) on every UI tick of the hold, then the relocation tick and two more at 1.0 |
| Hold | `sub_827A9C60` | VisualDirector: amount → `+48` |
| Build | `sub_827AAF10` | amount ≥ 0.0 (`0x82165A10`) → the presentation packet's header slot 3 carries it; after each build `+48` = −1.0 (`0x8216DEE0`) |
| Decode | `sub_827AB790` | presentation block `B+16`: `+148` = slot 3 present, `+152` = the amount (`B = *(*(0x83083C38)+0x2FCB4)`; reset `sub_827AB6E0`), i.e. `B+164` / `B+168` |
| Read | `sub_824DD6F0` | Class_Treatment update: `B+164` → w12 = 1, w13 = trunc(`B+168` × 10000); else w12 = 0 (w13 keeps) |
| Play | Treatments program | slots 1–12 while w12 = 1 (one every 30–130 ms), slot 0 when w13 reaches 10000 |

Measured (`audiox_marker_20261004_125320`, 0 malformed):
- **The 1559 m return** (hold 1.0 s, 61 `TPFX`): 15 crackles from +40 ms to +811 ms after the first message, then
  slot 0 5.5 ms after the go-to. Peak gains 0.02–0.28, slot 0 0.1229.
- **The 593 m return** (0.638 s) and a released hold (to 0.70) have it too; the released hold has no slot 0.
- **All 10 go-tos of three earlier sessions:** 3–5 crackles in each 0.2 s hold, slot 0 5–33 ms after the go-to
  (0.1229 on 10 of 12). Over 68 crackles: median peak 0.1870, max 0.2842.
- **The capture over the 1 s hold:** 250–4000 Hz rises 12–15 dB above a standing bed, peaks at 600–750 ms, almost
  nothing above 4 kHz.
- **Both marker returns loaded** (`TPSTREAM` 0 → loading state `0x82707508`; the mix is muted from ~+420 ms to the
  end of the load). The crackle does not depend on that: its trigger is the hold (amount present), and the distance
  sets only the hold's length (0.2–1 s).

Port: `crate::ui_audio::TeleportEffect` (session marker → the frame's amount; a mod's through
`sdk.audio.teleport_effect`), `PlayerAudio::teleport_effect` → `TreatmentGlobals { flag_164, value_168 }`. The screen
static reads the same resource. Proof: `teleport_crackle_follows_the_recomp` (onset, count, slot 0's time and gain,
median / max peak) and an e2e render of a standing hold (`E2E_TELEPORT`) folded like the capture: within 1.3–1.9 dB
per band over the hold. The bench is byte-identical. Not ported: the load's mute (no streaming load in our engine).
