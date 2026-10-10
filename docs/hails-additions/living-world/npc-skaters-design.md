# Living world: NPC skaters design (part of doc 26)

Status: design, 2026-10-04; milestone M1 (data) is done (see `skaters-data.md`). Retail findings and their evidence:
`.claude/notes/npc-skaters-re.md` (tags: [code] = retail code, [data] = disc data, [recomp] = observed in the
recomp; the code is the source of truth). Credit: the TU3 static recompilation (skate3recomp by @mchughalex, rexglue SDK, Xenia)
as the reference for how the retail code is used; no game code or data is copied, the disc data is read at setup like every other
asset.

**One PR (user, 2026-10-04): NPC skaters and all ped work ship upstream as one combined PR.** The phases below are
internal milestones of that PR. Parts marked **[shared]** are designed once for both NPC skaters and peds (and
later traffic); the ped design (`peds-design.md`) uses the same names.

## 1. What retail does (summary)
- A population manager keeps **3 ambient NPC skaters** (cap 5 AI total, 7 skater slots including the player)
  around the local player: every 2 s (60 world ticks) it may spawn one at the **start of an unused recorded line
  60-90 m away**, never within 5 m of another skater, picking line and character by a score with randomness; it
  despawns ambient skaters **beyond 120 m** (or 1000 m height). Challenges cap them at 2 or remove them.
- Each NPC is a **full skater**: same actor, same action/motion graphs, same board physics, driven by an
  `AIController` (path follower, obstacle avoider, on/off-board motion, nav-mesh walker, skater profile) through an
  AI physics input instead of the pad.
- They ride **recorded human lines** (3,891 ambient paths, 645 km, recorded at 60 Hz with positions, per-tick
  displacement, board/skater orientation, node flags, jump trajectories and trick slots; streamed per city tile;
  lines link into a network). None in the separate park districts.
- Identity: a `characters_marquee` record (ambient_skater_01..09 with recipes `ai_skater_01..09`, the pros and IP
  cast, teammates, community skaters) with an `ai_skater_profiles` record (trick weights, capability flags);
  characters are loaded into a 5-entry pool chosen to fit the nearby lines' types and pro masks.
- Audio and speech already ported (doc 11: one audible NPC board within 30 m of the camera, the bail grunt, pro and
  living-world voice lines). Peds react to NPC skaters' slams and collisions (`ai_spectateslam`,
  `ai_spectatecollision`).

## 2. Where our engine is
| need | what exists | gap |
|---|---|---|
| skater sim | `physics::SkaterRuntime` (a single `Resource`, ~136 files touch it) + `GamePhysics` (one board) | not per entity: running N full skaters needs a refactor |
| input | `skate_core::input::xbox` conversion, `PlayerInputRuntime`, `skate-data::input_recording` | no AI input source |
| animation | `graph_host` + stock graphs, `SkaterAnimation` evaluator | single instance |
| rendering other skaters | multiplayer `RemoteSkins` (skinned marquee/customiser looks from streamed poses, interpolation buffers), `Appearances` | keyed by network actor; reusable for NPCs |
| collision with other skaters | `physics::network::Proxies` (remote kinematic proxies join the solve) | keyed by network actor |
| models | `tools/asset_pipeline/native_roster.py` converts `characters_marquee` pros + IP | skips `ambient_skaters`, `teammates`, `community` |
| audio | `world_audio::NpcSkaterAudio` component, `game_audio::npc_skaters` host, `world_bridge` (inert until a system publishes), `NpcSkaterReactionEvent`, mod ghosts (`modding/world_audio.rs`) | needs a publisher |
| data | vault classes in `skater-collections.json` | no AIPATH decoder, no export |
| streams | district `cSim_*.xsf` region layers read in the audio export | AIPATH (0x00EB0014) not exported |

## 3. Architecture
### 3.1 Crates
- **`skate-data::aipath`** (new): parse an AIPATHDATA blob (record layout in the notes §3) into `AiPath { id, tag,
  bbox, flags, pro_mask, nodes: Vec<AiPathNode>, jumps: Vec<AiPathJump>, links: Vec<AiPathLink> }`. No I/O beyond
  bytes in. Format tests on synthetic blobs; data-gated tests on the disc export.
- **Setup export** (`tools/asset_pipeline`): `aipaths.py` writes `assets/.../living_world/skater_paths/<district>.bin`
  (our own compact format, or the raw blobs per tile with an index), plus `skater_profiles.json` (resolved
  `ai_skater`, `ai_skater_profiles`, `characters_marquee` rows) and extends `native_roster.py` to convert the
  ambient, teammate and (optionally) community recipes. **[shared]**: one `living_world` export group with the ped
  census/entities/models export.
- **`skate-core::living_world`** (new, pure, `#![forbid(unsafe_code)]`) **[shared]**:
  - `population`: a generic census engine: per kind a desired count, a spawn ring (inner/outer), a cull radius
    (horizontal/vertical), a phase cycle and a scorer trait. NPC skaters use `SkaterPopulation` (rules of
    `sub_8245BA28`, `sub_8245C548`, `sub_8245D520`); peds use the LW census rules (ring 50-60 m, cull 70 m, speed
    lerp, 1 spawn per tick) through the same trait. Deterministic: an explicit RNG (seeded) instead of `rand()`.
  - `skaters::path_follow`: the path cursor (node index, sub-node time at 60 Hz, link choice at the end, rejoin
    after a bail), returning the target pose/velocity/flags per tick.
  - `skaters::profile`: trick choice from `ProfileTrickEntry` weights for a recorded trick slot.
  - `skaters::character_pool`: the 5-entry pool and the character score of `sub_8245B068`.
  No ECS, no I/O: unit-testable against the code constants.
- **`skate-game::living_world`** (new Bevy plugin) **[shared]**: resources `LivingWorldSettings` (enabled per kind,
  counts, from data with mod overrides), `LivingWorldTick` (the 30 Hz console tick, frame-rate independent per the
  console-cadence rule), `PopulationState`; systems in `FixedUpdate` before physics; streaming of path tiles with
  the map's tile streaming (paths are per 100 m tile).

### 3.2 ECS model for an NPC skater
- Entity with `NpcSkater { id: u64 (stable), slot: u8, character: CharacterKey, profile: ProfileKey, kind:
  NpcSkaterKind (Ambient, Scripted, Mod) }`, `PathCursor`, `Transform`, `WorldAudio` pieces
  (`NpcSkaterAudio` filled by the NPC system), `Appearance` (marquee recipe look), `SkaterLod`.
- **Two fidelity tiers** (one component swap, same entity):
  - **Line replay tier** (`NpcSkaterReplay`): the skater follows its line kinematically: position, heading and
    board/skater orientation interpolated between 60 Hz nodes; animation driven from the node state (rolling,
    crouch, air with the jump trajectory, grind/slide/manual from the line's contact class, off-board) through the
    existing animation evaluator in a light "puppet" mode; a kinematic collision proxy (like the network proxies)
    so the player can bump into it. This is cheap and is what the player sees beyond ~30 m.
  - **Simulated tier** (`NpcSkaterSim`): a full skater (skate-core physics + graphs) with `SkaterInput::Ai(...)`:
    the path follower produces the AI physics input (steer toward the next nodes, push to the recorded speed,
    trigger the recorded trick slots at the recorded nodes, ollie at jump nodes with the recorded launch
    velocity as the target). Needed for retail parity close to the player (bails into objects and the player,
    collisions, audio from real contacts). Requires the `SkaterRuntime` refactor (§3.3).
  - Retail runs every NPC fully simulated; with ≤ 5 that is affordable. The replay tier is an engine choice for
    cost and for the first milestone, and stays as the far LOD (retail's `PhysicalPlayerHiLOD` hints at a LOD; open
    in the code). Switching rule: simulated within 40 m of the camera or when interacting, replay beyond; never
    switch while airborne/grinding.
- **Rendering [shared with peds]:** a `CrowdCharacter` renderer that reuses `multiplayer::render` (skinned
  look, pose buffer) keyed by entity instead of network actor; peds use it with their LW models.
- **Audio:** the NPC system fills `NpcSkaterAudio.state` from the simulated skater exactly like the local player
  (`skate_events::audio_state`), or a lite state (`AudioState::rolling`) for the replay tier (as remote players do);
  `list_order` = slot order; `voice` = `characters_marquee` voice id; reactions from the skater's events.
- **Peds [shared]:** NPC skater tricks, slams and collisions raise the same mood events as the player's (with the
  `ai_skater` perp filter) so peds watch and react (`ai_spectateslam`, `ai_spectatecollision`).

### 3.3 The SkaterRuntime refactor (needed for the simulated tier)
- Split `SkaterRuntime` + the player-owned parts of `GamePhysics` (board, riding outputs, clock state) into a
  per-skater bundle `SkaterSim` stored as a component; the local player keeps a `LocalPlayer` marker and the
  camera, HUD, scoring and input systems query `With<LocalPlayer>`. World data (collision world, grind world,
  materials, settings) stays shared in `GamePhysics`.
- Input: `SkaterInput { Pad(controller slot), Ai(AiPhysicsInput), Recorded(...) }`; the player input runtime runs
  on whichever source the skater has.
- Done in mechanical steps with behaviour-identity proof for the local player (skill regression-check §3b:
  identical state logs / e2e renders before and after), since the user warned about regressions.
- This is also the base for split-screen and better multiplayer (remote players could become simulated skaters).

### 3.4 Networking
- Host-authoritative population: the host's manager decides spawns/despawns; clients receive `(npc id, character,
  path id, start node, start tick, seed)` and run the same replay tier deterministically; simulated-tier NPCs near
  a client are replicated with the existing pose stream (`skate-net` pose/interpolation) like remote players.
- Retail shares 7 skater slots between players and AI; we keep the AI cap per session (3 ambient, configurable)
  and count remote players toward the 7-slot limit only when the retail-parity switch says so. Retail runs **no**
  ambient NPCs online (code, 2026-10-04: desired count 0 while the online flag is set), so the retail-parity default
  online is 0 ambient NPCs; a non-zero online count is a mod / setting choice, then host-authoritative as above.

## 4. Mod surface (designed in, not bolted on)
Mod-facing (Lua SDK API 2, `skate.living_world` **[shared]** namespace, cleanup on mod disable for everything a
mod spawned or overrode):
- Settings: `set_npc_skaters{ enabled, count, total_cap, spawn_inner, spawn_outer, cull, cycle }` (defaults =
  retail values from setup data/code constants, `nil` restores them).
- Spawn/despawn: `spawn_npc_skater{ character, profile, path | position+heading, tier }` → handle; `despawn(h)`;
  `list_npc_skaters()`; query distance like retail's `GetDistanceToNearestAISkater`.
- Content: mod-authored lines (node lists in the same schema as the decoded paths, or recorded from the player's
  own session: a "record a line" helper using the replay recorder), extra characters (marquee recipe or a mod
  model) and profiles (trick weight tables) through the content overlay (#36 pattern), mod "spots" (a list of
  lines tagged as a spot).
- Behaviour hooks: events `npc_skater_spawned`, `despawned`, `trick`, `bail`, `line_end`; a per-NPC override of the
  input (`set_input`), the line choice (`choose_line` callback) and the character choice.
- Engine-facing equivalents: Bevy events/resources with the same meaning (`NpcSkaterSpawned`, ...), so engine code
  and mods use one mechanism. Stable identities: character keys = `characters_marquee` record names, path ids =
  retail 64-bit ids, profile keys = `ai_skater_profiles` names.

## 5. Retail-parity plan
| item | parity source | status |
|---|---|---|
| counts, phase cycle, ring, cull, 5 m rule, scoring | code (`sub_8245BA28` family) | decoded |
| character pool and choice | code (`sub_8245B068`, `sub_82461D28`) | decoded except the pool's candidate list source |
| lines | data (AIPATHDATA) | layout mostly decoded; jump/link blocks to finish |
| path following, trick choice, avoidance, bail respawn, off-board | code (`PathController`, `ObstacleAvoider`, `PathRespawner`, `NavMeshController`, 0x82463200..0x82471188) | open: next RE step |
| tunables | data (`ai_skater`, 38 fields, sites known) | names open |
| audio and speech | done in #32/#44 | waits for a publisher |
| ped reactions to NPC skaters | data (mood results) | ped side |
Rule: values come from setup data or are code constants documented with their address; no own picks.

## 6. Test plan
- **skate-core unit tests** (no data): population rules with a scripted reference track (spawn ring bounds,
  5 m rejection, cap 3 / 5, cull 120 m / 1000 m, one spawn per cycle, cycle = 60 ticks at the console rate,
  frame-rate independence at 30/60/144 fps), path cursor (60 Hz interpolation, link continuation), character
  score.
- **Data-gated tests** (skip without assets): AIPATH decode over every tile: node displacement × ticks = node
  distance (ratio 1 ± 5 %), all nodes inside the bbox, 3,891 paths / 389,676 nodes; profile and character tables
  resolve (every `aiprofile` RefSpec hits a record, every ambient recipe converts).
- **Trace-gated headless sims:** feed the player's track from a recomp trace (SKATEB player board positions) into
  the manager with the real line data and compare against the same trace's NPC population: max concurrent 3,
  spawn distance p10 ≥ 55 m and p90 ≤ 92 m, despawn median 115-125 m (the recomp's tolerances, code wins on
  disagreement); NPC speed distribution vs the lines' recorded speeds.
- **Engine tests:** spawn/despawn leaves no entities, audio publishes `NpcSkaterAudio` for live NPCs only, mods'
  NPCs are removed on disable, the local player's behaviour is identical with NPCs disabled (regression-check
  state-log comparison), performance budget (3 simulated + replay NPCs within the frame-time target; skill
  optimisation measurements).
- **User testing:** in-game only (no comparison pages).

## 7. Phased plan (internal milestones of the one living-world PR)
| # | milestone | contents | size | shared with peds |
|---|---|---|---|---|
| M1 | data | `skate-data::aipath`, setup export of lines, profiles, characters; roster converts ambient/teammate recipes; data-gated tests | M (3-4 d) | export group, character conversion |
| M2 | population core | `skate-core::living_world::population` (generic) + skater rules + character pool; unit tests; trace-gated sim | M (3 d) | yes: the census engine |
| M3 | replay-tier NPCs | ECS plugin, line cursor, kinematic proxy, crowd renderer from `RemoteSkins`, puppet animation from node state, `NpcSkaterAudio` publishing, speech voice, debug overlay | L (1-1.5 w) | renderer, animation puppet, plugin |
| M4 | mod surface v1 | settings, spawn/despawn, events, content overlay for lines/characters/profiles, SDK docs + example mod, cleanup | M (3-4 d) | yes: one `skate.living_world` API |
| M5 | finish the RE | PathController → AI input, trick choice, avoidance, bail respawn, off-board/nav; tunable names | M (static RE, 3-5 d) | nav mesh with peds |
| M6 | SkaterRuntime per entity | refactor with identity proofs for the local player | XL (2-3 w) | no (but enables pros as peds-like actors later) |
| M7 | simulated tier | AI input source, LOD switching, collisions/bails with the player and peds, real contact audio | L (1-2 w) | ped reactions to NPC slams/collisions |
| M8 | multiplayer | retail default: 0 ambient NPCs online (code); opt-in host-authoritative population, deterministic replay tier, pose stream for simulated NPCs | M (3-5 d) | same replication path as peds |
Scope notes (2026-10-04 answers): parks get no NPC skaters; no on/off switch in career free roam (the Free Play mode's
"A.I. Skaters" option comes with Free Play, if ever); fixed time of day; zombie mode (cheat) turns ambient NPC skaters
off; the character pool is the pros plus recruited teammates (record byte +9), not `ambient_skater_01..09`, so M1
must convert the teammates' looks from the save / customiser data rather than only marquee recipes.
The PR can be opened as a draft after M4 (visible, moddable NPC skaters on recorded lines) with M5-M8 landing as
further commits; the PR description keeps a checklist per milestone.

## 8. Questions for the user (retail mechanics): answered 2026-10-04
1. ~~Options switch for AI skaters?~~ User: no AI skaters on/off option. **Code/data disagree for one mode:** the
   offline **Free Play** options screen (pause menu "Free Play: Disable career mode and play using custom options")
   has "A.I. Skaters" On/Off next to Traffic and Pedestrians; it only applies in Free Play (mode 3, `0x830B7AE8+340`).
   Career free roam has no switch. The Free Play mode, with this option, is in scope of this PR (user, 2026-10-04).
2. ~~NPC skaters in the parks?~~ No (user; the data has no lines there). Parks spawn none.
3. ~~Recruited team members in free roam?~~ **Yes (code + data):** recruited `teammate_01..04` are in the ambient
   pool with the pros (record byte +9), scored the same way; the Call Skater menu can also request them. The generic
   `ambient_skater_01..09` are NOT in the free-roam pool. Details: `npc-skaters-re.md` "Answers (2026-10-04)".
4. Online: no ambient NPC skaters (code: mgr+608 = online flag → desired 0). Time of day fixed. Zombie mode (cheat)
   turns ambient NPC skaters off (kill switch reads the zombie cheat).
