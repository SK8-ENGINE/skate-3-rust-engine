# 26e: Living world: NPC skaters: AI and simulated tier

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## NPC skater steering: the AI board path (M5 port, 2026-10-08)

**Problem.** Retail's NPC skaters are full physics skaters. Their AI (the PathController) writes a steering
record every tick, and the physics nudges the board toward it. Our engine had the record's path into the physics
input (`ProcessedPhysicsInput.external_physics_1616`) but not the step that acts on it, so a physics-bodied NPC
could not be steered. This change ports that step; NPCs still use the replay tier until they get a physics body
(the simulated tier, next).

**What retail does** [code, TU3; recomp disassembly as reference]:
- The record goes from the AIPhysicsInput component into the skater's physics state (`82593640`, +10512, "use
  external physics" +10688) and from there into the physics frame (`82DB4048` -> +1616, flags +1776; already
  ported as `publish_external_physics`).
- `UpdatePostPhysics 82D387A8` of `PHYSICS_STATE_PHYSICS_GROUND` (100) and `PHYSICS_STATE_SLIDE_GROUND` (101) runs
  the ground wipeout check `82D8F9E0`, then, while record flag bit 31 is set, the board path `82C05EC0`:
  - bit 30, position `82C056B0`: `d = (target - deck) * gain`; the deck (part 6 alone, `82D9C8C8` -> `82BD4318`,
    waking a frozen body with `82ADF7B8`) moves by `min(|d|, max_step)` along `d` when `|d| > 1e-6`
    (`0x830BD350`, initialised to 1e-6 by `82F826F8`);
  - bit 28, velocity `82C05868`: the deck body's linear velocity approaches the target velocity the same way;
  - bit 29, facing `82C05988`: the target forward in the ground frame (`controller+292` -> `+752`), y dropped,
    normalised, its signed angle from +Z about +Y (`8296EC98`) wrapped to [-pi, pi], times the gain, clamped to the
    maximum (degrees, `0x8206D110` = pi/180); the deck rotation becomes `D * F^T * Ry * F` and the whole board is set
    (`82C0B2C8`).
- Gains: the `physics_ai` vault record (`*(*(0x830CFDA4)+272)+4`, holder `8289D5C8` -> `8289D2E8`, class
  `527C93F55CFC663D`), `default`: velocity max change 0.2 m/s and gain 0.5 (+0 / +4), position max step 0.02 m and
  gain 1 (+8 / +12), facing max 2 degrees and gain 0.5 (+16 / +20) per tick. An `unstreamed` record (10 m, 1000 m/s,
  0.2) also exists; what selects it is not found yet.
- `PHYSICS_STATE_FOLLOW_PATH` (105) is a separate kinematic state (`82D43118` -> `82C05D78`: velocity from the
  position error, 40 m/s cap, 5 m/s change per tick); not ported yet. Jumps along the recorded trajectory:
  `82D682E8` / `82D67B50` / `82D67A00`, not ported yet.

**Change.**
- `skate_core::riding::grounded::state::board_path`: `PhysicsAiTuning`, `SteerTarget`, `approach`,
  `facing_step`, `rotate_about_ground_up`, `update_board_path` (retail order position, velocity, facing).
- `BoardRuntime::set_single_part_transform` (`82D9C8C8`: one part alone, frozen parts woken).
- `skate-game` `physics/board_path.rs`: runs after the ground wipeout check in PhysicsGround / SlideGround when the
  record's bit 31 is set; `physics_ai` loaded from the setup collections into `PhysicsSettings`
  (`SKATE_PHYSICS_AI` warning and the `default` record's values if the class is missing).
- The player's record never has bit 31 (its "use external physics" byte is 0), so the player's physics is
  unchanged.

**Files.** `crates/skate-core/src/riding/grounded/state/{board_path.rs, board_path_tests.rs, mod.rs}`,
`crates/skate-core/src/physics/board_runtime.rs`, `crates/skate-game/src/physics/{board_path.rs,
board_path_tests.rs, settings.rs}`, `crates/skate-game/src/physics.rs`.

**Verification.** skate-core `board_path` 4 tests (position and velocity steps and caps, the 1e-6 gate, facing
sign / wrap / clamp / gain / ground frame, the rotation about the ground up keeping the position, retail order and
per-flag gating). Data-gated skate-game test on DownTown (`SKATE3_ASSET_ROOT`, `SKATE3_MAP`): `physics_ai` loads from
the setup collections and equals the `default` record; 240 riding ticks with the player's record never steering;
a steering record on the riding board moves the deck 2 cm and its velocity 0.2 m/s toward the target, the wheels
untouched; without bit 31 nothing runs.

**Open questions.** The `unstreamed` switch; `physics_ai` +24 (0.95); FOLLOW_PATH and the recorded-trajectory jump.

## NPC skater trick choice (M5 port, 2026-10-08)

**Problem.** The replay tier always did the trick recorded on the line. Retail's ambient NPC skaters do not: in
their default behaviour an ollie or flip slot is re-picked from the character's AI profile, so the same line shows
different flips from different skaters and from one pass to the next. Not play-tested yet.

**What retail does** [code, TU3; read in the recomp disassembly, reference only]:
- The PathController's dispatcher `sub_8246A2E0` runs on every start-trick node (event 1) the skater passes. The
  trick's category comes from the static trick table `0x820862A8` (+16). Ollie / nollie (1) and flip (2) slots go to
  `sub_8246A080`; every other category (grinds, grabs, manuals, slides, reverts, plants) does the recorded trick.
- A higher chain level (heelflip2 / 3 / 4, kickflip2 / 3 / 4 and the nollie forms: table +8 base is another trick and
  not the ollie 128) takes no action: the base trick already running covers it. The late flips have the ollie as
  base and are not chain levels.
- Behaviour mode (behaviour context +60; `sub_824733A0` creates the ambient behaviour with mode 1):
  - 0: the recorded trick. `sub_82469FA8` walks the next start nodes within 120 frames whose trick's previous level
    (+4) is the current id, but it assigns that previous level each time, which is the id it already holds, so it
    always returns the recorded trick (an earlier research note read this as "takes the highest level"; the code
    does not).
  - 1 (ambient default): when the gate passes, a weighted pick from the profile, else the recorded trick.
  - 2: a scripted list (not ported, see Open questions). 3 and up: no ollie / flip.
- Gate `sub_82469C60`: walk forward from the slot summing each node's frames (+0x24) until 300. A start node first:
  pass only when it is an ollie / flip (jump table `0x82469D5C`: categories 1 and 2) whose previous level is the
  recorded trick, else fail. A landing (the airborne flag seen, then a node without it): fail when the next node is a
  start node, else pass only when more than 50 frames passed (the landing node's frames included; `subfc` / `eqv` /
  `addze` = signed `frames > 50`). The window end or the line end passes.
- Pick `sub_82469A28` -> `sub_8245FE58`: the nollie table (runtime profile +8) when the recorded trick's name
  starts with `n` / `N` (`sub_82469048`), else the regular table (+0). `u = rand32 / 2^32`; walk the 8-byte entries
  (weight, trick) adding weights (normalised to sum 1 at load, `sub_824720C8`) and take the first with `u < sum`,
  else entry 0. `sub_82469A28` also keeps the recorded trick when the trick's attribute record has no family entries
  in the per-category list; those records are not decoded, every recorded ollie / flip is taken to have them
  [inferred].

**Data.** The profile tables are already in the `livingworld` export (`skater_profiles.json`,
`ai_skater_profiles.*.fields`: `Hash_E580B6284639E03F` regular, `Hash_BB901D68361E9833` nollie, inheritance
resolved by `tools/asset_pipeline/living_world_skaters.py`) with each character's `aiprofile`; no setup re-run is
needed. The trick table's chain links (+4 previous level, +8 base) are 18 numeric rows in
`skate_core::scoring::catalog::LINKS`, read from the TU3 image and checked against the catalog (+0 = index and the
+12 / +16 columns match all 332 rows).

**Change.**
- `skate_core::living_world::npc_tricks`: `TrickMode` (recorded / profile / none), `TrickParams` (gate window 300,
  more than 50 frames), `TrickProfile`, `gate`, `pick_weighted`, `choose`.
- The line cursor asks `choose` at every start-trick node (`BranchContext.tricks`), keeps the running trick at a
  chain level, and emits `CursorEvent::Trick(TrickRecord {frame, line, node, recorded, chosen})`. `Decider::Mirror`
  takes the trick records next to the branch records, so a client (and the render look-ahead) applies the host's
  choices and never decides.
- Determinism choice (not retail): retail draws `rand32` from the skater's random source in tick order. Ours is
  `derive(npc seed, [line, node, frame])`: the same distribution, but a pure function of the NPC's seed and the slot.
- `skate-data`: `skater_trick_profiles` (tables by profile name, profile by character; `default` when a character
  has none).
- `skate-game`: each NPC's tables are its `aiprofile`'s, with mod tables applied; `NPC_SKATER_TRICK #id character
  node frame recorded chosen mode` in the log for every slot, `NpcSkaterEvent::Trick` for engine systems and the
  planned `sdk.living_world` events, `NpcReplay.tricks` holds the records a host would send. The puppet reads
  the cursor's trick (`resolve_trick_anim`), so the chosen flip's clip plays.
- Mod surface (`sdk.world.set_tuning("living_world", ...)`): `npc_tricks {mode, gate_window, min_air_frames}` and
  `skater_trick_profiles {[character key or profile name] = {regular, nollie}}` (lists of `{trick, weight}`; a key
  wins over a profile name per table; an absent table keeps the disc's). Cleared when the mod stops.

**Files.** `crates/skate-core/src/living_world/{npc_tricks.rs, npc_tricks_tests.rs, replay.rs}`,
`crates/skate-core/src/scoring/catalog.rs`, `crates/skate-data/src/living_world.rs`,
`crates/skate-game/src/living_world/{mod.rs, npc_skaters.rs, npc_tests.rs}`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/{world_tuning.rs, vm.rs, api.lua}`,
`sdk/skate.lua`.

**Verification.**
- skate-core `living_world::npc_tricks` 9 tests: the pick at the cumulative boundaries and the entry-0 fallback,
  the gate at 40 / 50 / 60 frames and with a mod window, a start node after the landing, the 300-frame window and
  the line end, chain continuation, nollie table only for `n*` tricks, other categories and modes, catalog links,
  the cursor emitting one record per slot (none at the chain level) and a mirrored client showing the same tricks,
  and the spread over 4,000 seeds (about 3 in 4 for a 1 : 3 table).
- skate-data: a unit test on synthetic JSON; data-gated on the user's export: all 193 profiles, every table entry is
  an ollie / flip (category 1 or 2) with a weight of at least 0, every pool character resolves to a non-empty table.
- skate-game: `living_world_npc_skaters_pick_tricks_from_their_profile_and_clients_mirror_them` (an ollie slot
  with 80 frames of air on every fixture line: picks only from the profile, both entries come up, a client cursor
  rebuilt from the records shows the host's trick; mode `recorded` keeps the ollie; the reset restores retail) and
  `npc_skater_trick_choice_set_merge_and_reset` (domain read, first writer wins, key over profile, invalid mode or
  trick id rejected, reset). skate-mods `world_tuning_commands_deserialize_and_validate` gains 4 cases.
- In game (muted DownTown run, 2026-10-08): 12 slots in the first NPC seconds; `360popshuvit` -> `kickflip` and
  `ollie` -> `fs360popshuvit` re-picked; grinds, grabs, manuals and short-air ollies kept the recorded trick.
- Full workspace run (`--lib --bins --tests`): only the 4 known upstream failures.

**Open questions.**
1. Mode 2 (scripted list, `sub_82469E10`) and who sets modes 0 / 2 (challenge or scripted AI skaters).
2. The trick attribute records' family entries (`sub_8245F120`, list at `*(0x830CFE64) + (category + 583) * 16`).
3. Profile byte +51 (grab / fingerflip / boneless allowed; one of the two unnamed bools) and the gesture-start
   percent (+36): only matter once the simulated tier sends ActionGraph signals.
4. Skater vfunc +36: whether retail's draw is a per-skater stream or the global generator.

## Simulated NPC skaters: per-skater physics context (M7 step 1, 2026-10-08)

**Problem.** Retail runs every ambient NPC skater as a full physics skater (the same physics states and board as
the player, steered by its AI record; see "NPC skater steering"). Our physics (`GamePhysics`) held exactly one
skater: its board, riding outputs and clock sat next to the shared world. About 800 places read those fields, so
splitting them out would be a large change with a regression risk for the player.

**Change.** A simulated skater's own parts are a `SkaterPhysicsContext` (board, riding outputs, clock, exchange,
tick count, carry, wipeout flags). An NPC skater keeps one and swaps it into `GamePhysics` around its own tick
(`swap_skater_context`, a plain swap of those fields); the collision world, props, grind world, settings and network
proxies stay shared, so moved props and every map change reach every skater. `new_skater_context(spawn)` builds one
from the same setup collections (kept in `GamePhysics`). Only the local player steps the dynamic props
(`owns_props`); NPC skaters push props through `actor_prop_volumes` as before, so props still step once per tick.
The world's query buffers are cleared on every query (no state carried between skaters).

Not yet: spawning simulated NPC skaters from the population, their AI record and pad, drawing them from their
simulated pose, the distance switch between replay and simulated, skater-to-skater collision.

**Files.** `crates/skate-game/src/physics.rs`, `crates/skate-game/src/physics/skater_context_tests.rs`.

**Verification.** Data-gated `the_player_is_bit_identical_with_a_simulated_npc_skater_in_the_same_world` on
DownTown: 300 ticks of the player pushing off, alone and again with a second skater (own context and runtime, 6 m
to the side, same input) ticking after the player every tick: the player's deck position and velocity bits are
identical on every tick; the second skater rode more than 1 m on its own board without falling through.

## Simulated NPC skaters: the AI record drives the physics (M7 step 2, 2026-10-08)

**Change.** A simulated skater gets its AI physics record through `SkaterRuntime.ai_physics` (`None` for the
player): the animation packet publishes it like retail `82593640` (record copied with the high flag bits only,
`82592810`; "use external physics" = the AI's fresh bit, "externally controlled" set), and the existing ported
publication puts it into `ProcessedPhysicsInput.external_physics_1616`. The record is built from the recorded line
(`skate_core::living_world::ai_record`, retail `sub_8246DB50` / `sub_8246DE38`): target = the path frame
(`path_frame`, board orientation, turned on board-flipped nodes) and position along the current segment
(`LineCursor::line_target`, no drawing blends), target velocity = the node's per-frame displacement x 60 (new
`ReplayNode::step`, node `+0x0C`; measured on the DownTown export: |step| x frames = segment length, median ratio
1.000 over 47,087 segments) clamped to 99.9 m/s, the frame's Ri / At negated when the skater faces the other way,
and the flags: bit 25 = on-board steering (seeded by `8246DB50` from `pc+922`), bits 31..28 from the state bytes as
decoded. Bit 25 matters: the ported state selector sends a skater whose record steers without it to
`PHYSICS_STATE_FOLLOW_PATH` (105), which is not ported yet; with it the skater stays in `PhysicsGround` and the
board path steers it.

**Verification.** skate-core `ai_record` 2 tests (flag rules incl. bit 25, target pose and speed, facing flip,
clamp). Data-gated `a_simulated_skater_rides_a_recorded_line_from_its_ai_record` on DownTown: a skater with only the
record (neutral pad) spawned on the first ground NPC line stays in `PhysicsGround` for 300 ticks and rides more than
5 m along the line, tracking it (median under 0.5 m, max under 1 m on the default line). The player identity test
still passes. skate-core `ai_record` also tests the spawn push (scale, kind 1, the node radius).

**Spawn push (retail, ported).** The first run fell behind the recorded speed (tracking error median 9.5 m, and the
slow board hit geometry). Retail starts a fresh NPC near line speed: `sub_824701F8` (hold branch) sets every board
part's velocity (`82C04168`) to the node's per-frame displacement x 60 x 0.75 (`0x821814A0`; x 0.5 `0x8209975C` for AI
kind 1 outside motion states 19 / 20) while the skater is within 0.01 m (`0x820D71E8`; 0.05 m `0x82165A00` past node
0) of its node in the ground plane (`ai_record::spawn_push`). With it, on the first seven DownTown ground lines the
simulated skater holds the recorded line over 5 s on its own physics: median error 0.06 to 0.2 m, max under 0.6 m, at
the recorded speed (about 8 m/s), except line 5.

**Open.** Line 5: around (-147.9, 10.18, 436.2) the board's contacts climb from 22 to 92, it rides about 3 cm up and
nearly stops, then recovers (max error 3.7 m) while the recording rolls through on the ground (no airborne node):
a ground-physics or collision difference at that spot (a curb, a prop, or our wheel solve), not the AI. Then:
FOLLOW_PATH (105), the recorded-trajectory jump (`82D682E8` / `82D67A00`), spawning simulated NPCs in the game and
drawing them from their physics.

## Simulated NPC skaters in the game (M7 step 3, 2026-10-08)

**Change.** `living_world::npc_sim`: an NPC skater within 40 m of the player, on the ground and not in a trick,
becomes a simulated skater (`NpcSim`: its own board context, `SkaterRuntime`, controls and camera runtime, loaded
from the setup data at the switch); beyond 44 m (1.1 x), at the end of its line or on a physics error it goes back to
the replay tier (only while on the ground). Each tick after the cursors advance: the AI record from the cursor, the
retail spawn push while on its node, then `GamePhysics::advance_npc_skater` (the player's frame with a neutral pad,
in its own context). It is drawn from its simulated pose like the player (`render_pose`, puppet root at the origin);
its population position and audio still follow the cursor. At most 3 at once; only the authority simulates.
Engine choices (retail simulates every ambient skater and never hands over): the distance switch and the handover at
the line's speed. Off by default until play-tested: `SKATE_NPC_SIM=1`, or the mod value `npc_simulated {enabled,
radius, max}`. Log: `NPC_SKATER_SIM` on every switch and every 2 s per replay-tier NPC with the reason it waits.

**Verification.** Muted DownTown run with `SKATE_NPC_SIM=1` (40 s): andrew_reynolds switched at 39.9 m and stayed
simulated for the rest of the run (about 30 s, through its tricks) without a physics error; the fixed physics step
went from about 2.3 ms to 4.4 ms with one simulated skater. The others waited "too far" (51 to 122 m). Not seen on
screen yet and not play-tested. skate-mods validation gains 2 cases; player identity and line tests still pass.

**Open.** Its recorded ollies and tricks are not performed yet (no trajectory launch, no ActionGraph trick signals:
a simulated NPC rolls through its jumps); skater-to-skater collision; how the board and look appear when drawn from
the simulated pose; render interpolation (drawn at the 60 Hz tick).

## NPC skater obstacle avoider (M5 port, 2026-10-09)

**Problem.** NPC skaters rode through peds, cars, props and the player standing on their line: retail's
`ObstacleAvoider` (AI controller `+80`) was not ported.

**Retail [code] (static decode, `.local/research/npc/m5-obstacle-avoider.md`).** Once per tick in the PathController
advance `sub_8246D560`: own speed, floor 0, cap FLT_MAX, mode 0; when the global avoidance switch is clear, four
gatherers add obstacles: skaters 64 m (`sub_82464968`, type 3), peds 8 m (`sub_82463C08`, type 2), vehicles 20 m
(`sub_82464000`, type 4), props 16 m (`sub_82464448`, type 5). The entry add `sub_82463A40` keeps an obstacle inside
40 deg of the facing (`0x822F91B4`), or inside 88 deg (`0x822F9554`) when closer than 1.8 m (`0x8208EB60`), at most
16. The fill `sub_824636B0`:
- closing rate and time to contact from the distance one 1/60 s step later (`sub_82462D00`);
- the lateral interval it blocks on the skater's path, in path widths: a circle of extent / 2 + skater radius
  (`sub_824629A8`), or the 8 box corners for vehicles and props (`sub_82462448`). Path widths are node bytes
  `+0x25/+0x26` / 50 (`sub_82F71580`);
- for a moving obstacle that blocks: a floor = speed to cross its line before it arrives + margin, and a cap = speed
  to arrive after it passed - margin (`sub_82463200`; margin [data] `ai_skater` `C373BAC23C5881FA` = 1.0, skater
  radius `8EF4FB9D11A9358A` = 0.5);
- a close blocker: gap < 2.2 m (`0x822570E0`), or < 6.0 m (`0x8208F74C`) inside 18 deg (`0x822F9284`); props 5.0 m;
- a skitch candidate: a car ahead moving the skater's way (both cosines > 0.8, `sub_82462ED0`).

Aggregation `sub_82464FA8`: highest floor, lowest cap; an entry without one invalidates it, and a close blocker
without a cap stops (cap 0); a floor more than 5.0 above the own speed is dropped. Targets: the earliest blocking
entry along the path, the skitch target with a 60-tick cooldown (`sub_824650A0`, `sub_82465198`), free gaps of the
path (`sub_82461DC8`). Mode `sub_82465578`: 4 skitch (`GrabWorld` signal), 3 steer when the path is split (the gap
nearest the skater's lateral position, its centre, `sub_82465280`; the target moves that far across the path,
`sub_82464E30`, controller `+933`), 1 speed up to the floor, else 2 slow to the cap; low props (under 0.8 m) give 6
when contact is within 1 s, a stop in front of a prop gives 7 within 1.5 s. The speed shape (`sub_82470830`) floors
(mode 1) or caps (mode 2) the AI record speed.

**Change.** skate-core `living_world::avoid` (pure, every value in `AvoidSettings`, retail defaults);
`replay::project_on_line` (nearest line point with the interpolated widths; `ReplayNode::width` from the disc).
skate-game `living_world::npc_avoid` runs before the cursors advance, on the host only: peds (NavPower agent radius
and height, velocity from the last tick), traffic cars (model bounds around the pose), props (collision boxes), the
players and the other NPC skaters. Replay tier: the cursor is held back by the missing recorded frames (a cap of 0
stops the skater; a floor only catches up held frames, a recording is never played faster), and the drawn skater
moves across the line towards the chosen gap at retail's AI board path step, 2 cm a tick (`82C05EC0`), back to the
line when clear. Air, off-board and trick spans are never held or moved. Simulated tier: the same cap / floor on the
AI record speed, the target moved across, controller `+933` set while steering. Log `NPC_AVOID` on every mode change
(mode, target kind and id, time to contact, cap, floor, lateral, lag); mod event `living_world` `npc_avoid`; mod
values `npc_avoid {...}` (all thresholds, radii, cones, margin, switch).

**Engine choices.** Retail has no replay tier: holding the cursor back and offsetting the drawn skater stand in for
the physics following the capped record. Retail's gather order is kept; the projection window of retail's path
search is not decoded (the gather radius is used). The skater obstacle size 0.75 (`0x821814A0`, read by the skater
gatherer) is inferred.

**Verification.** skate-core `avoid` tests (8): stop for a standing ped, cone and radius, speed up for a slow car
crossing (floor 9.0 m/s from the worked numbers), a floor over the headroom falls back to the cap, skitch target and
cooldown, free gaps, steering into the nearer gap and its offset, switch off. skate-game
`living_world_npc_skater_stops_for_a_player_standing_on_its_line`: a player standing 3 m ahead, 0.5 m right of the
line stops the NPC (under 3 m in 3 s instead of 22 m, `npc_avoid` slow_down event), which rides on once the player
leaves. skate-mods validation cases for `npc_avoid`. Not play-tested.

**Open.** Mode 6 / 7 consumers (low prop ollie, stepping off: logged only), skitching itself (mode 4 raises nothing
yet), the prop gate `sub_82464350`, an entry-add offset term reading controller `+128`, retail's path search window.

## NPC skater proxies in retail's collision groups (2026-10-09)

**Problem.** The NPC proxies used contact group 0 (world), so the player's skeleton contact pass treated an NPC like
a wall: no `SkaterSkeletonScalar` (0.5) on the force and no `SkaterSkaterThresholdScalar` (1.5) on the player's
thresholds, so the player bailed about 3x too easily against NPC skaters.

**Retail [code] (`.local/research/npc/m7-skater-collision.md`).** Skater against skater is an ordinary skeleton
contact: `sub_82BD4A30` branches on the other body's group (5 skater, 8 vehicle, 4 ignored) at `0x82BD5388`; group 5
scales the force by `AISkeletonScalar` 1.0 (AI) or `SkaterSkeletonScalar` 0.5 and keeps the other skater's id; the
player's ground thresholds rise by `SkaterSkaterThresholdScalar` 1.5. Already ported
(`skeleton_body/collision_update.rs`, `player/wipeout/ground.rs`); the multiplayer peers' proxies already use it.

**Change.** `npc_skaters::proxy` returns two solids like the peers: the body capsule in group 5, the board box in
group 4 (`PROXY_BOARD_BIT` on its id). Test `living_world_npc_proxy_audio_and_clips` checks the groups.

**Open.** The NPC side of the contact (retail: its own skeleton contact and wipeout check with the AI scalar): only a
simulated NPC can bail; the replay tier does not react. The other skater id in the feedback block is not filled.

## Simulated NPC skaters bail and respawn (M7, 2026-10-09)

**Retail [code] (`.local/research/npc/m5-ai-bail-trigger.md`, `m5-path-respawner.md`; main checked the key
addresses).** The physics step writes `flags2468` bit 18 (the "Wipeout" graph attribute) to skater component `+59`
(`sub_82DB6EC0`); the PathController copies it into `pc+924` each tick (`sub_8246EF78`); the rising edge enters the
respawn (`sub_8246EE30`): delay 5.0 s for ambient AI (`0x821F1790`), clamped to 1.5 (`0x822249B4`) to 7.9
(`0x822572EC`) s; after it the skater is placed at a node of its current path (path state 8) and the spawn push
applies; no fade. Skater+1904 bit 0x02000000 is not a bail flag ("an object spawned on me this frame",
`sub_826C09E0`); `m5-path-controller.md` called it "bailing", corrected.

**Change.** `npc_sim`: a simulated NPC whose wipeout bit rises bails; its line waits (the cursor is held back) and
after the delay a fresh simulated skater is placed at the line target (placement and spawn push as at the switch).
Mod values `npc_simulated {respawn_seconds, respawn_min, respawn_max}`; events `npc_bail` / `npc_respawn`; logs
`NPC_SKATER_BAIL` / `NPC_SKATER_RESPAWN`. Test `living_world_npc_respawn_delay_is_retails_and_clamped`.

**Open.** Retail's node choice `sub_8246ED68` is only partly read (we resume at the cursor's place); the "prepare"
step 1 s before the respawn (freezes the ragdoll, `pc+29`); not seen in a game run yet (the simulated tier is opt-in).

## Simulated NPC skaters do their recorded jumps and tricks (M7, 2026-10-09)

**Problem.** A simulated NPC skater rolled through the jumps and tricks on its line: its pad is neutral and nothing
told its graph to pop, and the physics had no recorded arc.

**Retail [code] (`.local/research/npc/m7-ai-recorded-jumps.md`, `m7-ai-trick-signals.md`; main re-read
`sub_824551C0`, `sub_824691A0`, `sub_82D682E8`, `sub_82D67B50`, `sub_82D67A00`, `sub_82D68C80`).**
- Signals, not pad input: the PathController writes named ActionGraph signals into the intent map the player's pad
  listener fills. Anticipation (`sub_824691A0`, every tick): within 3.0 m of the line (`0x82063B08`) it looks up to
  60 recorded frames ahead for a start-trick node; an ollie or flip there (category 1 / 2) gives `AnticMag` 1.0 and
  `AnticAngle` 0, or pi for a nollie (the recorded trick); `sub_824696B8` posts them while the skater faces along the
  line (`Crouch` on crouched nodes, `Manual` before a manual). Dispatch (`sub_8246A2E0`): on the tick the committed
  cursor reaches the start-trick node, `Trick`, the trick's scorable name, `GestureSpeed`, `TrickHeight` and
  `DontMirrorTrick` = 1.0; only the newest event node, none when more than 3 nodes were crossed in one tick.
- The record (`sub_8246DA18`): a node whose ext data has HasTrajectory puts its arc into the AI record: +80 start
  position, +96 start velocity, +112 gravity (0, -9.8, 0) (`0x822F8B40`), +128 the -1 splat, +144 offset; bit 26;
  bit 27 when the jump lands in a grind or slide (`sub_824551C0`: walks on from the trajectory node, category 5
  start-trick yes; manual / powerslide, incidental air, a ground end-trick, another trajectory more than 60 frames on
  or more than 600 frames in all no; after an airborne end-trick the first ground node answers by its next node).
- Take-off (`sub_82D682E8`): with an AI record (`F+2472` 0x20000000) the selector first tries the recorded arc
  (`sub_82D67B50`): the board one step after take-off within 2 m of the recorded start (4.0 m^2, `0x82257308`), moving
  with the recorded velocity, speed ratio 0.333 to 3.0 (`0x82093DA0`, `0x82063B08`; unchecked below 0.0001 m/s). If
  accepted it casts that one arc (`sub_82D67A00`: count 1, flag 9659, `TrajectoryRadius`, start and end error 1.0
  (`0x8231A844`; the research file named the word 8 bytes off), duration max_time); the computed start velocity
  stays. `sub_82D68C80` then returns at once: no grind-to-middle lock, no second pass. Otherwise the normal computed
  arcs. Landing: a recorded jump that does not lead into a grind may not lock to a grind middle (already ported,
  `air/trajectory/scoring.rs`).

**Change.** skate-core `living_world::ai_signals` (anticipation, crouch, manual, dispatch of the cursor's chosen
trick, `SignalSettings`); `ai_record::{with_trajectory, lands_in_grind, upcoming_trajectory}`;
`air::trajectory::RecordedArc` in `SelectorInput` (only with an AI record and bit 26, so the player's path is
unchanged), `launch::accepts_recorded` and the one-arc batch, the selector skips the grind lock and the second pass
after a recorded arc; settings `recorded_arc_*` (code constants kept as data). skate-game: `npc_sim` builds the
signals each tick and `GamePhysics::advance_npc_skater` inserts them where the player's gestures go; the record
carries the next recorded take-off ahead of the cursor. Mod values `npc_simulated {anticipation_distance,
anticipation_frames, max_crossed_nodes}`. Log `NPC_SKATER_SIM_TRICK` (trick, node, physics state).

**Engine choices.** Retail reads the take-off from the PathController's current ext entry (`pc+692`, cursor not
decoded); we use the next take-off node ahead of the cursor over ground nodes, guarded by the 2 m accept rule. The
posting gates of `sub_824696B8` (heading `pc+824`, `pc+926`) are taken as "facing along the line" (inferred).

**Verification.** skate-core tests: `a_recorded_jump_lands_in_a_grind_only_per_retails_scan`,
`a_recorded_trajectory_fills_the_record_block`, `a_recorded_arc_is_accepted_only_near_its_start_and_speed`, the three
`ai_signals` tests. Full skate-core and skate-game runs: only the known pre-existing failures.
Muted DownTown run with `SKATE_NPC_SIM=1` (120 s): 11 tricks dispatched to simulated skaters (andrew_reynolds:
fs360popshuvit, tailmanual, ollie, nosemanual, bsrevert, n_heelflip; lucas_puig: n_360inwardheelflip, kickflip,
kickflip_underflip), the skater in `KnownAir` within half a second of an ollie (it popped); one bail (andrew_reynolds)
and its respawn on the line 5.00 s later at node 52, then tricks again; no panic, error or physics error; 81
`NPC_AVOID` lines. Whether each take-off cast the recorded arc is not logged yet. Not play-tested (opt-in tier).

**Open.** The meaning of `pc+824` / `pc+926` / node flag 0x02 in the posting gate, which `SetTrickHeight` branch
`TrickHeight` 1.0 takes, which velocity the body leaves the ground with on a recorded arc, timing against a trace.

## Avoider modes 6 and 7: the controller sub-modes (2026-10-09)

**Retail [code] (`.local/research/npc/b66-npc-modes-6-7.md`, `b70-npc-submode-reposition.md`; main checked
`sub_8246F818` cases 3 / 5 and that the line reset `sub_82468AC8` writes 0 to `ctrl+568`).**
- Mode 6 (a prop lower than 0.8 within 1 s): `sub_8246D560` sets controller sub-mode 4. While 4 the recorded node
  action is dropped, the speed shape is skipped and no grab or trick is posted; the skater keeps driving at its
  target. Nothing ends it except a line reset (the line's end, a junction switch to another line, no line), mode 7 on
  a later tick, a reposition or a full controller reset.
- Mode 7 (blocked by a prop within 1.5 s, cap under 0.1): tick 1 sets sub-mode 3 and a one-shot reposition request; a
  reposition hands the skater to a second controller (`sub_824661A8`) at a safe node on its line (`sub_82455728`: the
  first of 3 calm nodes). That controller is retail's "NavMeshController": the skater steps off and walks there (see
  "Avoider mode 7: stepping off and walking back to the line" below; corrected 2026-10-10, b78 to b80). If the plan
  fails, a second tick in mode 7 sets sub-mode 5 and posts `WipeOutRequest` once (the skater bails).

**Change.** skate-core `living_world::ai_controller` (sub-mode values, `enter_low_prop`, `step_off`,
`reposition_node`, `take_reposition`, `reposition_done`; retail values as `ControllerSettings`) and
`ai_signals::drop_trick_dispatch`. skate-game: `NpcAvoid` keeps the controller state and resets it on a line change or
the line's end; mode 6 enters sub-mode 4, which skips the speed shape (replay and simulated tiers) and drops the
simulated skater's trick dispatch.

**NOT RETAIL YET.** Mode 7 is wired for simulated skaters only (section below). The replay tier keeps its recorded
tricks in sub-mode 4. The chooser's mode-7 gates `S+71` and `+6007` are not modelled.

**Verification.** skate-core `ai_controller` (4) and `ai_signals` tests; skate-game npc / living_world 80 pass. Not
play-tested.

## Avoider mode 7: stepping off and walking back to the line (2026-10-10)

**Retail [code + data] (`.local/research/npc/b78-mode7-controller-b.md`, `b79-controller-b-outputs-path.md`,
`b80-ob-steer-consumer.md`; main checked every constant, the throttle `82467B00`, the steer `82467C90` with its angle
`8296EC98`, the reset `82466F40`, the toggle presses `824712E0`, the writer slots `82470D08` / `82470D78`, the reposition
`8246FE38` and the safe node picker `82455728` with `82455348` / `824545F0`).**
- The reposition picks the node (cursor + 5 or the prop's node, + 2, a remembered prop node, clamped), then the safe
  node `82455728`: from there, the first of 3 nodes in a row with more than 45 recorded frames since a jump marker (a
  ground trick start resets it), more than 15 since a flag-0x4 node, a step slope under 1 and trick class 0, 4 or 8
  (`82455348`: a ground trick = grind / slide, manual, powerslide, or the ground grabs coffin and gnd_*grab).
- The brain swaps to controller B (debug name "NavMeshController"; A is "PathController"). B plans a NavPower path
  (the runtime the peds use) to the node: with waypoints the last one within 2.0 h / 1.0 v of the node, without
  them the node within 1.5 h / 3.0 v of the skater.
- Each tick B writes the player's on-foot intents, never position or velocity: `NewToggleOffBoardState` /
  `ToggleOffBoardState` every 5 ticks while the skater is still on the board ("ToggleOffboardState", byte 161 =
  SkaterOffBoard), `OB_Steer` (heading error x 6/pi, full lock at 30 degrees; turns in place when it has not moved
  for 10 ticks), `OB_Mag` (0.5 x distance^2 to the aim point, capped at 1, a 0.3 floor when facing it), and
  `WipeOutRequest` when stopped or stuck (600 ticks on one waypoint, or 90 ticks without moving or turning).
- The AI walks in the biped's direct mode (the external-controller flag: `OB_Mag` and `ob_Turn` = `OB_Steer`, no world
  stick). B hands back to A at the node (within 2.4 h / 1.0 v, heading within pi/8 of the line), when no plan is left,
  or on abort / wipeout; A re-attaches by its saved line id. A direct plan (no waypoints) hands back on its first tick:
  the heading is still zero after the reset and the angle of a zero vector is 0.

**Change.** skate-core `living_world::controller_b` (`ControllerB`: activate / plan / tick, the stuck monitor, the
toggle presses, steer and throttle; `ControllerBSettings` with every retail value; `PathService` with `DirectPath` and
the peds' `NavMesh`, whose funnel corners become the path elements) and `ai_controller::safe_node`. skate-game: the
simulated tier runs mode 7 (`step_off`, `take_reposition`, `safe_node`, `ControllerB::activate`), ticks B instead of
the AI record while it is active (the AI source stays present but not fresh, so the biped keeps its direct mode),
re-spawns the line cursor at the node on the hand-back and resets the sub-mode; `PlayerControls::ai_driven` keeps the
neutral pad's analog fill from overwriting the AI's `OB_Mag`. Logs `NPC_STEP_OFF`, `NPC_WALK_BACK`; mod event
`npc_walk_back {id, node, started}`; mod values `npc_simulated {walk_back, walk_arrive_distance, walk_stuck_ticks}`.

Retail simulates every ambient skater, so a replay-tier skater that a prop blocks (mode 7) becomes a simulated skater
at any distance (within the tier's cap) and walks round; it goes back to the replay tier once it rides again out of
range (`NPC_SKATER_SIM ... blocked by a prop (mode 7)` log).

**NOT RETAIL YET / open.** Needs the simulated tier (opt-in `SKATE_NPC_SIM=1` until play-tested); with it off, a
blocked skater still just stops. Our
path elements are funnel corners (retail's 56-byte elements carry two points per portal); B's heading `B+112` is the
skater root's forward and P the root position ([inference] for `[rec+20]+0` / `+416`); the "can move" terms
`[rec+72]+308 / +309` are not identified; the reposition clock is our tick count (retail's clock unit is open).

**Verification.** skate-core `controller_b` (8: plan acceptance, steer and throttle shape, arrival with the one-tick
heading lag, the direct plan's first-tick hand-back and the steer sign, the stuck wipe-out, the toggle cadence, online
/ abort) and `ai_controller` (5, incl. `safe_node`); skate-game data-gated
`controller_b_steps_off_and_walks_the_skater_onto_the_node` (DownTown: off the board at tick 6, `OB_Steer` reaches
Processed +2680 as -OB_Steer, the heading error goes from -1.57 to 0, hand-back "arrived" at tick 134); skate-core
living_world 241, skate-mods 105, skate-game living world / NPC / modding / skater context / carry / prop 180. Not
play-tested.
