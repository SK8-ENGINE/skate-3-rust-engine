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
  in the per-category list. That check always passes: `sub_82DA1BF0` puts every record into its category's list
  (it skips only a missing record) and all 105 ollie / flip ids have a record (research b187 [code + data]; main
  re-read the list loop).

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
2. Settled (b187): the family entries check always passes for ollies and flips (see above). Still open: the name of
   the record value +124 (`Hash_D34A84B044B60CE3`, 0 or 1) that the check counts by.
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

**Retail [code] (static decode, a local research note).** Once per tick in the PathController
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

**Open.** Mode 6 / 7 consumers (low prop ollie, stepping off: logged only), the prop gate `sub_82464350`, an entry-add offset term reading controller `+128`, retail's path search window.
(Skitching itself was ported later: 26h-living-world-traffic.md#skitching-step-7-npc-skaters-skitch-opt-in-2026-10-09.)

## NPC skater proxies in retail's collision groups (2026-10-09)

**Problem.** The NPC proxies used contact group 0 (world), so the player's skeleton contact pass treated an NPC like
a wall: no `SkaterSkeletonScalar` (0.5) on the force and no `SkaterSkaterThresholdScalar` (1.5) on the player's
thresholds, so the player bailed about 3x too easily against NPC skaters.

**Retail [code] (a local research note).** Skater against skater is an ordinary skeleton
contact: `sub_82BD4A30` branches on the other body's group (5 skater, 8 vehicle, 4 ignored) at `0x82BD5388`; group 5
scales the force by `AISkeletonScalar` 1.0 (AI) or `SkaterSkeletonScalar` 0.5 and keeps the other skater's id; the
player's ground thresholds rise by `SkaterSkaterThresholdScalar` 1.5. Already ported
(`skeleton_body/collision_update.rs`, `player/wipeout/ground.rs`); the multiplayer peers' proxies already use it.

**Change.** `npc_skaters::proxy` returns two solids like the peers: the body capsule in group 5, the board box in
group 4 (`PROXY_BOARD_BIT` on its id). Test `living_world_npc_proxy_audio_and_clips` checks the groups.

**Open.** The NPC side of the contact (retail: its own skeleton contact and wipeout check with the AI scalar): only a
simulated NPC can bail; the replay tier does not react. The other skater id in the feedback block is not filled.

## Simulated NPC skaters bail and respawn (M7, 2026-10-09)

**Retail [code] (local research notes; verified the key
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

**Retail [code] (local research notes; re-read
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

**Retail [code] (research b66, `b70-npc-submode-reposition.md`; verified
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

**Retail [code + data] (research b78, `b79-controller-b-outputs-path.md`,
`b80-ob-steer-consumer.md`; verified every constant, the throttle `82467B00`, the steer `82467C90` with its angle
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

## The stuck tracker: stalled NPC skaters recover in stages (2026-10-15)

**Problem.** In the user's play session of 2026-10-15 (NPC Skater Physics on) two skaters stalled next to their
line, rolling at 0.2 to 0.3 m/s for 20 to 30 s (the user saw a skater stuck against a little pillar for
about 15 seconds before it reset and skated off). Our only recovery was the "lost" test, which needs the
skater more than 6 m from its line (`path_follow.rs`, `lost_radius`).

**Root cause.** Retail has a second, independent recovery we had not ported: the stuck tracker `sub_82468240`
(block `pc+528..+583`), called from the projection `sub_8246C930` with the projection's progress flag. [code, b111]
Its stage is the controller field `pc+568`, the same field our avoider code already calls the sub-mode
(`ai_controller::sub_mode`).

**Change.**
- `path_follow.rs`: the projection's `progress` flag (b110): false when cursor B is ahead of the candidate, A is
  past the start and the raw projection advanced less than `progress_fraction` (0.025) of A -> B this tick.
- `ai_controller.rs`: `track_stuck` (the tracker), `stuck_action` (the per-tick reader `sub_8246F818`),
  `resnap_node` (`sub_8246D168`), `path_reset` (`sub_82468AC8` clears the block on a line switch) and `full_reset`
  (`sub_82468C10` after a placement). A no-progress tick (after 120 ticks since the last full reset, on the board
  and not airborne) starts a stall and climbs the stages past 30 / 30 / 2 / 180 / 30 / 3 ticks: 2 re-snaps the line
  cursor once, 3 raises the walk-back request (5+ nodes ahead, through the existing reposition), 4 holds, 5 posts
  `WipeOutRequest` (a bail), 6 places the skater at the respawn node. The stall clears after more than 25 progress
  ticks once the skater is more than 5 m (horizontal) from where it stalled. All values are in
  `ControllerSettings` (data-driven, mod-overridable); the state is per skater and tick-counted.
- `npc_sim.rs`: runs the tracker after each projection, the stage actions before it, and the full reset after a
  bail respawn too ([inferred]: retail's respawner places through the same `sub_8246E558`; without it a stage 5
  bail would repeat after the respawn). Log: `NPC_SKATER_STUCK #<id> <name> stage <a> -> <b> at node <n>`, the
  re-snap and the stage 6 respawn.

**Verification.** skate-core `living_world` 308 pass (new: `a_skater_that_never_moves_climbs_every_stage`,
`a_stall_clears_after_25_progress_ticks_more_than_5_m_away`, `untracked_ticks_and_the_line_start_rule`,
`stage_4_moves_on_to_the_bail_after_180_ticks`, `resets_keep_or_clear_the_gate`); skate-game 704 pass (known
upstream setup failure only). Passive 4-minute runs (NPC Skater Physics on, muted, no input): lucas_puig stalled at
node 70, stage 2 after 1.4 s, re-snapped to node 69 and rode on; a line switch no longer starts a false stall.

**Open.** A skater held at a speed cap of 0 behind a pedestrian that does not move (jerry_hsu, 80 s in the second
run) is not a tracker case: cursor B does not lead A, which retail counts as progress. What ends that wait in retail
is being researched (b112). Medium confidence on the re-snap details (b111).

## Avoider: steer around a partial blocker, and the real closing speed (2026-10-16)

**Problem.** In a passive run (2026-10-15) the NPC skater jerry_hsu stood still behind a pedestrian for more than
80 s: the avoider held a speed cap of 0 (`slow_down ... cap 0.00`), switching between steer and slow_down.

**Root cause.** Two differences from retail [code, b112, verified both in the recomp]:
- The path-split flag `+6006` is "free lateral gap count != 0" (`sub_82465198` tail at `0x82465268`: `addic` /
  `subfe` on `+64`, the free interval count that starts at 1 for [-1, 1]), and `sub_82465578` goes straight to
  mode 3 (steer into the gap, `sub_82465280`, no speed limit) when it is set. Ours used `gaps.len() != 1`: a ped
  near one edge (one free gap) slowed the skater where retail steers, and a fully blocked path steered.
- The cap only exists while closing (`sub_82464FA8` reads entries with the closing flag, set by `sub_82462D00` from
  both velocities). Ours gave the avoider the recorded line velocity, which stays non-zero while the cap holds the
  cursor, so closing never fell to 0 and the cap held for good.

**Change.** `avoid.rs` `pick_mode`: `path_split = !gaps.is_empty()`. `npc_avoid.rs`: the skater's velocity for the
avoider is its actual motion (the sample's position change per tick; a jump over 2 m falls back to the recorded
velocity for that tick). Retail reads the skater object's velocity (vtable `+16`; that it is the body velocity is
[inferred]).

**Verification.** Tests updated to retail: a ped leaving a gap is steered around and two peds closing the width
stop the skater (`standing_ped_close_ahead_stops_the_skater`); the speed up / slow down tests run with the whole
width blocked; `living_world_npc_skater_steers_around_a_player_standing_on_its_line` (was "stops for"). skate-core
and skate-game pass apart from the known pre-existing failures. Passive 4-minute run: no skater stood still for
more than a second (steer 41, slow_down 14, speed_up 11 mode changes).

## Recorded grinds and slides: the input schedule and the takeoff board yaw (2026-10-16)

**Problem.** In the user's 2026-10-15 session the followed NPC skater mark_appleyard landed 4 of 4 recorded grinds as
another grind (a recorded bs_noseblunt as a 50-50, a bs_tailslide as a 5-0).

**Root cause.** Retail never forces the grind family. It gives the AI skater the inputs a player would give and lets
the normal grind investigation pick the family (research b113, main
re-checked every address below in the recomp):
- **Dispatch per category** (`sub_8246A2E0`, jump table 0x8246A45C): only ollies and flips (categories 1 / 2,
  `sub_8246A080` -> `sub_82469448`) post `Trick` + the trick name. A grind (5), nocomply (7), hippy jump (10),
  footplant (13) only keep the trick in the slot (`pc+844`); fingerflips, grabs and boneless (3 / 4 / 12) do so only
  when the AI profile's byte +51 is set (layout +400 = `Hash_6DBAF7AD6CDE6A16`, copied by `sub_8245FFD8`; layout
  offset from the skaterschema), else the slot keeps the previous trick; a revert (11, `sub_8246A1E8`) posts `Trick`
  and `SlideFs180` (fsrevert) / `SlideBs180` (bsrevert); handplant (6) and manual (8) only validate. An end node or a
  failed dispatch clears the category (0x8246A6D4). Ours posted `Trick` + name for every category.
- **The schedule** (`sub_824696B8`, jump table 0x824698F8 -> `sub_82470ED0`): every tick from the start node, the
  slot trick's `scoring_trick` `SkaterIntentTuple` list (up to 6 entries: intent index, value, first and last tick,
  last < 0 = until the slot ends) is posted for categories 1-5, 7, 10, 12, 13; 6 posts `GrabWorld` 1.0. Data: a 5-0
  holds `PhysGrindUpDown` -1, a nosegrind +1, grab variants `Left/RightGroundGrab`; 50-50s, noseblunts, tailslides,
  boardslides have an empty list. Intent names: the executable's enum table (0x82FCB260, 146 names).
- **The takeoff board yaw** (`sub_8246B1F8`): on the takeoff edge, d = heading(recorded board forward) -
  heading(recorded skater forward) of the node the jump lands on (corrected 2026-11-01, see [below](#board-yaw-reads-the-landing-node)), where `sub_8296EC98(v, +Z, +Y)` is the acos of the
  normalised dot with +Z, made `2 pi -` it when `cross(v, +Z) . +Y < 0` (so `atan2(-v.x, v.z)` in [0, 2 pi); 0 for a
  zero vector; node matrix row 2 = the quaternion applied to +Z, `sub_82453970`). d is wrapped to [-pi, pi], folded
  to [-pi/2, pi/2] (nose and tail on the same line); above 0.31416 rad (18 deg) `BoardAdjustAngle` = +-pi/2 by the
  sign of d and `BoardAdjustMag` = |d| x 2/pi (`0x822F8B7C`, 0.63662: 90 deg = 1.0, the full FLAT clip; not clamped; we used 6.0606 from the neighbouring `0x822F8C7C` until 2026-11-02, research b801). The air arm (`sub_8246B358` -> `sub_82471008`) posts
  them every airborne tick, after the yaw steer; with a manual anticipated ahead it posts -2.677 (nosemanual) / 0.257
  instead, magnitude |0.5|. This was the open sign: decoded, no guess left.

**Change.** `skate-core/src/living_world/ai_signals.rs`: `TrickSlot`, the per-category dispatch, `slot_signals` (the
writer), `retail_heading`, `BoardYaw` (`takeoff`, `post`), `BoardYawTuning` (retail defaults), `manual_value`,
`anticipated`. `skate-core/src/input/skater_intent.rs`: the intent enum names. `skate-data/src/scoring.rs`: each
scorable's `intents` from `Hash_F216FED54DCBEB50` (raw array items now kept by `collections.rs`).
`skate-data/src/living_world.rs`: `slot_tricks` per profile. `skate-game/src/living_world/npc_sim.rs`: the slot and
board yaw on the NPC (cleared with the controller), the board yaw latched at takeoff and posted in the air arm (on by
default and separate from the air steer switch), the writer after the dispatch gates (skitch, off the path or board,
the placement hold skip it; low props clear the slot). Logs: `NPC_SKATER_SIM_SLOT` (slot opened without a perform),
`NPC_SKATER_BOARD_YAW` (takeoff yaw outside the dead zone).

**Moddability.** The schedules are the scorable records' data (a content override of `scoring_trick` changes them);
`npc_simulated {board_yaw, board_yaw_dead_zone, board_yaw_mag_scale}`; the profile byte comes from
`skater_profiles.json`. Slot and yaw are plain data on the NPC (serialisable, deterministic, no RNG).

**Verification.** Unit tests in `ai_signals.rs` (grind slot and schedule until the end node, schedule windows and
category arms, revert keys and the profile gate, the heading against `atan2(-x, z)`, the fold and dead zone, a
noseblunt posting nothing at dispatch); `skate-data` `stock_grinds_carry_their_ai_intent_schedules` on the user's
assets (5-0 -1, nosegrind +1, noseblunt and 50-50 empty). Exactly 90 deg sits on the fold edge, where the byte
rounding of the recorded frames decides the side (as in retail). Not yet measured: recorded vs landed grinds in a
session (needs NPC Skater Physics on near a grinding line; compare `NPC_SKATER_SIM_SLOT` with the landed GRIND lines).

**Powerslide (9), 2026-10-29 (research b676).** The writer arm is case 9 of the per-tick writer `sub_824696B8`
(0x8246999C), not `sub_8246DB50`: while on the path (`pc+920`, before this tick's drop) it posts `RightSlideStart`
1.0 when the ground-steer error `pc+824` < 0, else `LeftSlideStart`, and `LeftSlide` -0.42 / `RightSlide` +0.42 every
tick; dispatch posts nothing for 9. Ported: `ai_signals::slot_signals` takes a `Slide` (on path, heading error,
`PowerslideValues` as data, `npc_simulated.powerslide`); the sim passes this tick's ground-steer error and `on_path ||
left_path`; the ground steer gets `category_9` from the slot. Test
`a_powerslide_writes_its_start_side_and_both_slides_on_the_path`. Open: our heading error's sign against retail's
`pc+824` (a flip would swap the start sides); not seen in game. The flip retry `sub_824695A0` (HoldPattern) and the takeoff branch of
`sub_8246A700` that posts `WipeOutRequest` (key 0x830BECC4) and clears `pc+844` when `sub_82460460` agrees are not
ported.

## Bumps: the player and simulated NPC skaters knock each other down (2026-10-16)

**Problem.** The user (2026-10-15 session, confirmed for retail 2026-10-16): bumping NPC skaters should knock them
down. Ours: a simulated NPC was left out of the player's solve (2026-10-15, its replay proxy had made it collide with
a copy of itself), and the player was not in the NPC's solve, so nothing could knock anybody down.

**Retail.** Both are full skaters: a skater touching another is an ordinary skeleton contact of collision group 5
(`sub_82BD4A30`, 0x82BD5388), the force scaled by `AISkeletonScalar` for an AI skater and `SkaterSkeletonScalar`
otherwise, the non-AI side's ground thresholds by `SkaterSkaterThresholdScalar`; each side's own wipeout check decides
(research `m7-skater-collision.md`). No skater-specific bail reason. Anger and attacks: no code or data for path AI
skaters (research b125: the collision mood poster `sub_8269D8A8` reads the ped brain at +5896; the `ai_skater`
living-world entity is a stub; the mood reaction sets list `skater_male` / `skater_female`). The skaters that warn,
chase and take the player down are the skater peds (`skater_male01..03`, `skater_female01..03`, category
`ped_class_skater`, b126), which our ped census and mood system already spawn and run; male ones use the
reaction sets `skatercollisionwarn:2` / `skatercollisionchase:1` [data] (b125 reports from the recomp: the first bump
gives a warning, the second an angry chase and the takedown; not re-checked), dropping a carried board before
the chase (`pedestrian_newchasee.xml`); female ones taze or flee.

**Change.** `npc_skaters::push_proxies` adds each simulated NPC's own body after its last step (board and skeleton,
`BodyState`, like a multiplayer peer: `Proxies::append`) at the end of the player's proxies and records where they
start (`GamePhysics::npc_sim_proxy_mark`); `npc_sim::run_steps` drops them from each NPC's own solve
(`Proxies::truncate`) and adds the player's body and the other simulated NPCs instead. `network::capture_body_of`
captures any context's board. Switch: `npc_simulated.bump` (retail on; `SKATE_NPC_BUMP=0` turns it off).

**Verification.** `physics::skater_context_tests::a_hard_bump_knocks_a_simulated_npc_skater_down` (DownTown, ignored
test with assets): a skater riding at 9 m/s into the standing player wipes out at tick 26 (the player, shoved by the
sliding NPC, at tick 36); at 2 m/s it touches (41 contacts) and nobody falls. The game wiring compiles and the suites
pass; feel in game is for the user's session.

## Bumped NPC skaters: no anger, a collide line; pros stay calm (2026-10-16)

**Question.** In retail, do NPC skaters get angry when the player bumps them? The user remembered riding and walking
NPC skaters reacting, and skaters attacking and using tasers (unsure of the details), and that the pros standing at
challenge spots do not get mad.

**Answer from retail code** (research b127; verified points marked):
- Path AI skaters (riding the recorded lines) cannot warn, chase, take down or taze. The chaser component (vtable
  `0x8232C198`) is built only by the ped constructor `sub_82E33198`; the skater actor constructor `sub_82590DC0` does
  not build it, it only gets the chase-group (chasee) component [code, verified both]. The mood brain is a ped
  object (`ped+5896`) and `livingworld_entities_moodreactions` has no AI skater set [code + data].
- An off-board AI skater (ToggleOffboardState, an AI controller part) walks as a skater actor: no ped brain, mood or
  chaser [inferred from component lists, medium-high]. Its trigger is open.
- What a bumped path skater does: it bails (already ported, see "Bumps" above) and, when it is not the local player,
  says a skater collide line. `sub_824BDA90` (called by the body poster `sub_824BC188` at `0x824BC354`) sets three
  flags from skater sound entry floats `+596` / `+608` / `+600` against two tuning values (keys `0x7503A1AD356BFCCD`,
  `0x359EC8EF52598AA8`, fallback `0x830D0850`) and, off the race modes 19 / 20, posts living-world event 8228
  `130_collide` with arg 3 (`li r29,8228` at `0x824BDD38`, verified); no flag = 8318 (none). The takes are skater
  voices only (`130_89_sktm1_col` etc.). The audio state's `+596` / `+600` / `+608` come from the bridge `sub_824B0DA8`,
  which copies 116 bytes from the skater entry `+320` to the audio state `+496` (so entry `+420` / `+424` / `+432`)
  [code, verified]; per b128 entry `+320` is the collision conditioner block (`sub_82773298`), a copy of the
  Collision object's `+80`, so the inputs are Collision `+180` / `+184` / `+192` of the current frame.
  Writer [code, main 2026-10-16]: the tail of `sub_82BD60C8` (r8 = Collision `+80`) copies SkeletonCollision `+4056`
  -> `+180`, `+4048` -> `+184`, `+4052` (a word) -> `+188`, `+4060` -> `+192`. Those are written by the
  SkeletonCollision Update `sub_82BD4A30`: zeroed at its start, then per contact report a running max of the contact
  speed by the other body's group: group 8 (cars) -> `+4056`, group 5 (skaters, times the skater / AI skeleton
  scalar; `+4052` = the other entity) -> `+4048`, group 11 -> `+4060`. Our port already has these as
  `maximum_group_8_force` / `maximum_skater_force` / `maximum_group_11_force`
  (`crates/skate-core/src/physics/skeleton_body/collision_update.rs:122-143`). So the line reads: car contact speed
  (`+596`) > 0.16, group 11 (`+608`) > 0.2, skater (`+600`) > 0.2 (keys verified in `sub_824BDA90`).
  Thresholds [data, values verified 2026-10-16]: `aud_speech/default` 0.16 (`0x7503A1AD356BFCCD`) and 0.2
  (`0x359EC8EF52598AA8`). (Corrects an earlier note here that tied them to `sub_8252EDD0`: that object is camera
  tuning.) Correction to b128: the body poster's `+260 + 4i` is the region cooldown, not a previous value, so the
  check runs for every region the poster reaches (an impact, no cooldown), before the material bands.
  The post [code, main]: `sub_824A9A20` finds the skater's speech record in the `+708` list and calls `sub_824DAC00`
  with the pair (8228, main-cast 3); in game modes 19 / 20 (8250 or 8240 by `rand() & 1`, 17); nothing for
  (8318, 294).
- The warn / chase / takedown and the tazers the user remembers match the skater-looking peds (b126): `skater_male`
  peds warn, drop the board and chase; `skater_female` peds taze or flee. Both are already ported via the ped mood path.
- Pros at challenge spots: their graph `livingworldentities/marquee/marquee.xml` (30 lines) is AI > Marquee
  (DisableNavigation, LockToCurrentPosition) with only Colliding (NeedsToBeginColliding / IsPedestrianColliding),
  TrackingSkater (IsFacingSkater / ProTrackSkater) and HoldAtSpawn (ProWaitingForSkater). It has no MonitorEnvironment
  and no AddressWants, which `pedestrian/Pedestrian.xml` runs, so a pro stumbles on a bump but never gets a mood
  [data, verified 2026-10-16]. Our collision rules already give `marquee` no knock-down
  (`skate-data/tests/ped_anim_data.rs:112`). Standing pros are not spawned by our engine yet (branch plan: challenge
  hosts, spawn positions open); when they are, they must run the marquee graph, never the pedestrian graph.

**Change (2026-10-16).** NPC skaters say the `130_collide` line when bumped:
- `skate-audio` `AudioState::collide_vehicle` / `collide_skater` / `collide_other` (`+596` / `+600` / `+608`), filled
  by `skate_events::audio_state` from the skater's SkeletonCollision maxima.
- `Contacts::body` (the body poster) counts `collides` for a non-local skater when a region posts and any of the
  three is over its floor (`ContactsTuning::collide_vehicle_min` 0.16 / `collide_min` 0.2, exported by
  `tools/asset_pipeline/audio_export.py` as posters `collide_vehicle_min` / `collide_min`; the retail values are
  the defaults until a setup refresh exports them).
- `NpcSkater::take_collides` -> NPC host `collides` -> world speech: `skater_speech::collide` gives the message pair,
  which plays through the skater's speech record like the bail grunt.
- Tests: `player::contacts::tests::a_bumped_npc_skater_asks_for_the_collide_line`,
  `world::skater_speech::tests::a_bumped_skater_says_its_collide_line`.
- Only simulated NPC skaters have a skeleton body, so replay-tier NPCs (no physics contact) never say it.

**Crash line mute after a skater bump (2026-10-18, b267).** Retail [code]: on the skater flag (`+600` over its floor) a
non-local skater's `sub_824BDA90` walks the speech records (`+708` list) to the one with the skater's model (`+76`,
active `+52`) and writes `+148` = 3.0 (`0x82063B08`, at `0x824BDDD8`); `sub_824F9FC8` counts it down by dt, and
the crash poster `sub_824DB688` posts the crash message 8229 / 125 only when `+148 <= 0` (`fcmpu` against 0.0 at
`0x82165A10`, else a jump to `0x824DB7D0`). The announcer request after it and the `+193` latch run either way, so
a crash inside the 3 s stays silent for its whole length. A car or ped contact alone does not set the timer. Ours:
`Contacts::skater_collides` counts the skater contact's collide lines, the speech host calls
`SkaterSpeech::mute_crash` for that skater (by id; retail matches by model, which would also mute another skater
with the same model) and `SkaterSpeech::tick` counts down per console frame. The 3.0 is
`ContactsTuning::collide_crash_mute`, poster key `collide_crash_mute` (a mod sets it through
`sdk.audio.set_tuning("player", ...)`). Tests `a_skater_collide_mutes_the_crash_message_for_3_s_but_not_the_announcer`,
`only_a_skater_contact_counts_as_a_skater_collide`; skate-audio 268 pass, skate-game game_audio + living_world 183
pass. Not play-tested.

**Open (follow-ups; b267, derived in research, not yet verified
unless marked).** Corrections to the list above it: 24630 `423_Collide` / 24698 `474_BailOffBoard` are
announcer lines (speaker `*(system)+1036`, 104 in free skate), not main-cast; 8290 `210_CollTeam` goes to the bumped
skater's own speech record when it hit the local player (`+604` == the local index, `+492` 1.0 s cooldown, verified:
`0x8231A844` = 1.0), with a voice slot (`+28` record `+80` > 0); the `*(system)+1036 < 104` / `sub_824F2DF0` gate
guards a separate host comment (main-cast 254 `311_Coll_Pro` for a pro bumper, else 248 `312_Coll_skater`), not 8290.

**Team line 8290 (2026-10-18, b270).** Ported: the skeleton contact now carries the other body's owner
(`SkeletonContactReport::other_entity` / `SkeletonCollision::other_skater`, a stable peer id from
`Proxies::actors`, filled in `skeleton_feedback.rs`; retail `+604`), `AudioState::collide_skater_local` = that owner
is `PLAYER_PEER`. The body poster keeps one `CollideCall { skater, team }` per call; `entry+492` is
`Contacts::coll_team_cooldown`, counted down by dt at the start of every call and set to 1.0 when a skater contact
from the local player asks for the team line. The speech host posts `ContactsTuning::coll_team` (8290 / 294) on the
skater's own voice when it has no main-cast words: our stand-in for retail's AI voice slot (`+28` record `+80 > 0`,
slots map to the living-world models 89..96) [inferred: a pro's slot 0 is not proven]. Tuning keys
`coll_team_cooldown`, `coll_team` (posters, mod-settable). Test
`the_team_line_needs_the_local_player_and_a_cooldown_counted_on_calls`; skate-core 1116, skate-audio 269, skate-game
801 pass. Not play-tested.

Still to port: the host comment (needs our challenge host as a ped speaker, unverified) and the local player's
branch: 32797 `718_40_collide_skater` (cameramanspeech.big, not exported by our setup) is silent in free skate
(b270 [code 0x824ACD00]: the gate reads the raw host model, 104 in free skate), then announcer 24630 / 24698 by
on-foot `+716`. Open: whether retail NPC skaters ever get ped contacts (`collide_other`, needs a trace).

**Data-driven / mod-overridable:** the two thresholds (tuning by key), the event id per game mode, the voice sets, and
which graph an entity type runs (graph path per entity). Multiplayer: the host decides whether a bump plays a line,
keyed by the NPC skater's stable id.

## Replay-tier skaters skip grabs for profiles without slot tricks (2026-10-16)
- **Problem:** the replay tier played every recorded grab, fingerflip and boneless; the simulated tier already skipped
  them for profiles without the slot-trick flag, so the same NPC did different tricks in the two tiers.
- **Retail [code]:** the AI's trick dispatcher `sub_8246A2E0` runs categories 3, 4 and 12 only when the profile byte
  +0x33 is set (`Hash_6DBAF7AD6CDE6A16`, false in 44 of 193 profiles, mostly pros; research b159 item 1, re-read
  the switch).
- **Change:** `npc_tricks::TrickContext::slot_tricks` (from `LivingWorldTricks::slot_tricks_for`, the same data the
  simulated tier reads); `choose` starts no trick for those categories without it.
- **Verification:** `a_profile_without_slot_tricks_skips_grabs`; skate-core living_world 325 pass. Not play-tested yet.
- **Open (b159):** the early trick fire (record +144), the per-character line check at branches (`sub_82456970`,
  ported 2026-10-29, doc 26c),
  the powerslide input arm (ported 2026-10-29, above).

## NPC skaters react to wipeouts near them: slam, slam_b and crash lines (2026-10-16)
- **Problem:** the NPC skater speech process (`skate-audio` `skater_speech`, its reaction bytes) was only ever fed by the
  mod API (`NpcSkaterReactionEvent` from `sdk` world audio): in a normal game NPC skaters never said their slam,
  slam_b or crash lines.
- **Retail [code] (research b165 / b166 / b168; verified the entry header `mulli 544; addi 240` of `8279A2D0`, the
  per-update reset `clrlwi 7` of R+0x24 in `8245A7E8` and the radius constant `0x822F8F68` with its strict compare in
  `8245E950`):** the reaction manager `8245A7E8` runs once per simulation tick (tick mask bit 0, `8285C928`). It
  clears every skater's reaction bits, then each skater's wipeout start (the wipeout byte rising, `82772748` H+0xD1;
  the byte is Processed `+2468` bit 18, `82DB6EC0`) sets slam on every other skater closer than 15.0 (squared compare
  against 225.0) with "by" = the sender's model (`8245E6C8`, `8245E740`, 97 when there is none); the wipeout end (H+0xD2)
  sets slam_b the same way (`8245E848`); skaters are visited in index order, so the last sender names "by". The packer
  `827A1B78` copies the bits into the speech record's entry (+148 crash = the wipeout byte itself, a level; +152 bits
  4 / 3 slam / slam_b; +156 the "by" ids).
- **Change:** `skate_core::living_world::npc_reactions` (pure, deterministic: `ReactionState::step` over the skaters
  in stable order, previous wipeout per key); `npc_sim::reactions` (after `npc_sim::simulate` in the world tick): the
  players (observers, model 0, not listening) and every NPC skater (its voice model; the simulated tier's wipeout bit,
  the replay tier never wipes out) send `NpcSkaterReactionEvent` Slam / SlamB / Crash, which the bridge already holds for
  a console frame. Host only. Mod values `npc_simulated.reactions` (on) and `npc_simulated.reaction_radius` (15 m);
  the mod event entry stays as a second writer.
- **NOT RETAIL YET / open:** trick seen (score > 50, `Hash_4BEEDCA4AD799796` [data]) waits for the writer of the score
  at (P+0x3C)+0x88; the skater order is ours (players first, then NPC skaters by id); remote players send "by" 0; the
  call rates of the edge writer / packer / speech reader against the manager's tick are not proven (b168 section 3).
- **Verification:** `a_wipeout_start_and_end_reach_skaters_within_15_m_for_one_tick`,
  `an_npc_crash_is_a_level_and_the_last_sender_names_by` (skate-core), `npc_skaters_react_to_the_players_wipeout_near_them`
  (skate-game wiring: slam by the player at 10 m, nothing at 20 m, one tick only, slam_b at the end); skate-game
  living_world 93, world_tuning 24, world_audio 5, skate-mods world_tuning 3 pass. Not play-tested yet: wipe out
  near an NPC skater and listen for its line.

## The steering record targets cursor B; the handover logs its start speed (2026-10-17)

- **Problem:** a laptop session (DownTown, #38 mark_appleyard) showed a skater stalling right after the replay ->
  physics handover, then leaving and rejoining its path several times a second. Research b174 compared our projection
  with retail and found the on / off path test and the projection identical (the flapping is retail's own loop for a
  skater that does not move), but one real difference in the steering record.
- **Retail [code] (b110 2.6 / 2.7, m5 path controller):** `sub_8246DB50` builds the record with `sub_8246DE38` from
  cursor B of the same tick, after the projection: B's interpolated node frame, B's position and B's per-frame
  displacement x 60, speed-shaped. After a rejoin B = A + one shaped tick, just ahead of the skater.
- **Ours before:** the record target was the replay cursor taken at the top of the tick (one tick late), and that
  cursor only moves forward to B, so after a rejoin behind it the target stayed up to about 1 m ahead of retail's.
- **Change:** `replay::line_target_at(line, (node, t))` (the pose at any line position; `LineCursor::line_target` now
  uses it), and `npc_sim` builds the record (speed shaping, avoider offset) from `PathFollow::b` after `tick`. The
  replay cursor stays for drawing, trick signals and the spawn push. The handover line now reads `NPC_SKATER_SIM #id
  name <- replay (distance, node N, push P m/s)`, and a new one-off line `NPC_SKATER_SIM #id name start speed V m/s
  after the handover (push P m/s, deck D m from node N, state S)` shows whether the queued teleport keeps the push and
  whether the deck lands within the 0.01 / 0.05 m spawn push radius (both open in b174 section 4).
- **Open:** whether the stall itself came from the old target is unproven; read the new start speed line in the next
  session with a handover. Values stay data (`PathTuning`); the spawn push factor and radii are still constants
  (`ai_record.rs`), listed in the follow-ups.
- **Verification:** `steering_target_at_cursor_b_matches_the_cursor_pose_like_sub_8246de38` (skate-core); skate-core
  living_world 338 pass; skate-game living_world 95 pass (4 ignored). Not play-tested yet.

## Handover only on a safe node (2026-10-17)

- **Problem:** laptop session game-20261017-133408: 6 of 19 replay -> sim handovers lost the spawn push; 4 of them
  (#48, #54, #60, #74) were handed over at node 33 of one line, one node before a recorded bs_lip (node 34, logged
  0.04 s later, e.g. #48 19:40:15.278 / .317), and #48 then sat in GrindFiveO at 0 m/s and got stuck (research b178).
- **Retail [code] (b178):** an AI skater is only ever placed on a node returned by the safe-node pick `sub_82455728`
  (a run of calm nodes: no recent jump / flag, no steep slope, no trick class other than 0 / 4 / 8 ahead).
- **Change:** `npc_sim` hands a replay skater to physics only where `ai_controller::safe_node(line, node) == node`;
  otherwise the every-2-s report says `not a safe node`.
- The hold and push order is the next section.
- **Verification:** skate-game living_world 95 pass (4 ignored); muted 75 s DownTown run: three handovers (nodes 28, 42,
  49), start speeds 5.4 / 6.4 / 7.2 m/s = their pushes. Not play-tested yet.

## Handover hold and spawn push (2026-10-17)

- **Retail [code] (b178):** a placement resets the controller's hold counter (`pc+888` = 30 in the reset `82468C10`,
  8 after a bail respawn in `8246EA90`); `824701F8` counts it down one per tick and, while it runs, sends a record
  with no steer and runs no path branch; the spawn push (`82C04168`) fires only on the counter's last tick, when the
  skater point (`82467FC8`) is still within 0.01 m (first node) / 0.05 m of its node and the node is not off board.
- **Ours before:** `place` set the push as the velocity at the placement, every placement held 0.5 s, and the path
  branch re-tested the push every tick from the deck, which sits 0.20 to 0.46 m off the node (all 19 laptop
  handovers), so it never fired again.
- **Change (user 2026-10-17: the handover holds one tick, functionally the same as retail, no visible stop):**
  - placed at rest (zero velocity); the hold counts only once the placement teleport has landed;
  - on the hold's last tick the push (`ai_record::spawn_push`) is applied once when the skater is still within the
    radius of where it stood when the hold started (retail: of the node it was placed on), not on off-board nodes;
  - the per-tick push in the path branch is gone (retail has none there);
  - hold lengths: one tick at a replay -> sim switch (`HANDOVER_HOLD_TICKS`, engine handover), 30 ticks after a
    reset placement (stage-6 stuck respawn, `npc_simulated.placement_hold_seconds` 0.5 s), 8 ticks after a bail
    respawn (new mod key `npc_simulated.respawn_hold_seconds`, 0..=10 s).
- **Open:** byte `+0x1B6` (pauses the count with on-board steering), which body point `82467FC8` returns.
- New log `NPC_SKATER_PUSH #id name node N <speed | none: left its place> (moved d m since the hold started)`.
- **Verification:** skate-game living_world + world_tuning 118 pass, skate-mods world_tuning pass; muted 90 s
  DownTown run: handovers push 5.4 / 6.4 / 7.1 m/s on their first held tick (start speed = push), a bail respawn at
  node 55 pushed 5.7 m/s after its 8-tick hold. Not play-tested yet.

### Update 2026-11-03: no path projection during the hold

- **Problem [log]:** our path projection kept running during the hold, so the cursor moved off the placed node and the
  push read the wrong node's off-board bit and step. Four University respawns at node 62 (laptop session
  game-20261101-101251) all logged `NPC_SKATER_PUSH ... node 64 none: left its place` and rode on at half the recorded
  speed until they drifted 1 m off the line and bailed again. A DownTown session on 2026-11-03 showed it too: placed at
  node 27, pushed from node 28. Research b807.
- **Change:** the projection branch in `living_world/npc_sim.rs` also waits while the hold runs, as retail's hold
  branch (`824701F8`) does. The cursor stays on the placed node, so the spawn push uses that node (retail `pc+0x330`),
  and the projection starts from it once the hold ends.
- The `NPC_SKATER_RESPAWN` line now gives the time since the bail (it always printed 0.00 s before).

### Update 2026-11-03: the respawn at 5.0 s

- **Problem [code + log]:** every bail -> respawn took 8.55 to 8.58 s, where retail places at 5.0 s. Ours waited for the
  skater's own wipeout-end teleport. An NPC carries the externally-controlled bit (flags2472 bit 28), so its wipeout
  auto-resets only after a hard-coded 8.0 s plus the 0.5 s `TeleportAutoResetFadeOutTime`
  (`skate-core/src/player/wipeout_state/recovery.rs`); retail has the same 8.0 s rule (`82D3E980`). Research b810.
- **Retail [code]:** the respawner `8246EA90` places the skater itself once t = ticks / 60 passes the delay (5.0 s,
  clamped 1.5 to 7.9 s, below the 8.0 s reset), without looking at the wipeout. The placement `8246E558` queues the
  skater's teleport (`825926F8`); the teleport request wins over every other physical state, WipeoutGround included,
  which ends the wipeout (research b811). On board unless the node is an off-board one (node bit 3).
- **Change:** `living_world/npc_sim.rs` places the bailed skater on the delay through the actor reply (the same
  placement as the fallback before), on board from the node's off-board bit; the bail respawn and the stage-6 stuck
  placement both use it. The wipeout-end teleport to the chosen node stays as a backstop, and a warning still fires if
  a skater is not placed 10 s after the delay.
- **Verification:** skate-game 995 passed, data-gated living_world 7 passed. Muted University idle runs, 300 s each,
  player off the board: before, 3 respawns at 8.58 s, each pushing from its placed node (node 24, 6.7 m/s); after,
  5 respawns at 5.00 to 5.02 s, 4 pushes from the placed node (the fifth came as the run ended), no wipeout within 1 s
  of a placement, no late reset to an old checkpoint (the failure of the earlier 5 s attempt in doc 26c), 0 panics.
  Not play-tested yet.
- **Open:** retail's prepare step 1.0 s before the placement (world collision off: Player+0x510 bit 24, board group 2,
  the skeleton's driven mode 1, `82BE3808`) is not ported; ours never sets that flag. Why a skater placed on a nose-down
  node slides 0.23 m during the hold (retail's placement transform `82453AD0` not read yet).

## A dropped trick slot shows no trick pose (2026-10-17)
- **Problem:** user (laptop session game-20261017-143646) said NPC ollies and landings look wrong. The log has 698
  trick slots: 465 played as recorded, 215 swapped by the profile pick, 18 dropped (`chosen none`, grabs on profiles
  without slot tricks).
- **Retail [code] (research b187):** the swaps are retail. In ambient mode the gate `sub_82469C60` decides, and on a
  pass `sub_82469A28` takes a weighted pick from the profile's regular or nollie table (by the recorded name's first
  letter); there is no family, stance or pop class rule. Our `npc_tricks::choose` matches it. A dropped slot starts no
  trick at all (`sub_8246A2E0`).
- **Ours before:** a dropped slot still opened the trick span (`replay.rs` START_TRICK arm), so the replay phase was
  GroundTrick / AirTrick with no trick clip and the puppet fell back to `G_5050_FS_LOW_0_CYC` (a 50-50 grind pose) on
  the ground or `IA_IDLE_LO_N_0_CYC` in the air.
- **Change:** a recorded trick the choice dropped opens no span; the record (`chosen -1`) is still logged and mirrored.
  A START_TRICK node without a trick slot keeps its old behaviour (open below).
- **Tests:** `a_dropped_slot_opens_no_trick_span` (skate-core npc_tricks tests); skate-core living_world 341 pass.
  Not play-tested.
- **Open (b187):** whether retail's AI jump follows the recorded trajectory (`82D682E8` / `82D67A00`): a swapped trick's
  `_A` clip is cut at the recorded landing whatever its length, which may still look wrong; whether a START_TRICK node
  without a trick slot opens anything in retail; a `Continue` (chain level) with no open trick plays the recorded
  chain level here, retail may start nothing (not verified).

## Simulated NPC skaters hold a manual on its side (2026-10-17)
- **Found while chasing** the andrew_reynolds stall at DownTown node 60 (line 006477746e0017fab069ea024b7c9244: a
  tailmanual at node 56, its end at 57, an ollie at 58 over a ~0.24 m step). A per-tick trace showed `Manual` posted
  only before node 56 and nothing while the manual was open.
- **Retail [code] (read in the recomp):** `sub_82469338` gives a manual's side: the tunable `742E...` (default
  `0x830D0850`, 0.5), positive for nosemanual (2), negated for tailmanual (3), else 0. The anticipation arm of
  `sub_824696B8` posts `Manual` = that value for the manual ahead (`pc+788` = 8, trick `pc+792`); the dispatcher
  `sub_8246A2E0` stores it at `pc+836` for a manual (near 0x8246A544); the per-tick writer's category 8 arm
  (`loc_82469954`, jump table 0x824698F8 case 7) posts `Manual` = `pc+836` every tick until the end node.
- **Ours before:** anticipation posted `Manual` 1.0 (the nose side, also for a tailmanual) and the writer posted
  nothing for category 8, so the manual was dropped at its start node.
- **Change:** `SignalSettings::manual_value` (0.5, the one source; the board yaw's manual side reads it too),
  `TrickSlot::manual` (`pc+836`), anticipation and the category 8 writer post the side. Mod key
  `npc_simulated.manual_value` (0..=1). `SKATE_NPC_TRACE_TICKS` (default 60) lengthens the per-tick NPC_SKATER_PLACED
  debug trace after a placement (`RUST_LOG=info,skate3rust::living_world::npc_sim=debug`).
- **Tests:** `a_manual_is_anticipated_and_held_with_its_side` (skate-core ai_signals); skate-core living_world 342
  pass; skate-mods world_tuning pass. Muted 150 s DownTown runs before / after: bails 5 / 6, final stalls 5 / 6, lost
  22 / 17 (noise; sessions diverge). Not play-tested.
- **Not fixed by this:** andrew_reynolds still posts `Trick ollie` at node 58 and never leaves the ground (2 of 45 sim
  ollies / flips in the run fail, both this spot). Speed is fine (7.2 to 7.7 m/s), the early fire does not apply
  (b188: ollies have no lead), and 13 other ollies popped with no anticipation at all.
- **Cause found (2026-10-17 ~17:30, per-tick trace with the graph states, `ag` / `mg` on NPC_SKATER_PLACED):** the
  motion graph sits in `Bumped` from the tick after the spawn push for about 38 ticks (0.6 s); every placed NPC in the
  run does it (#1 to #7, 25 to 39 ticks). `IsBumped` (`82BA7310`, `ground_acceleration.rs`) reads the deck's observed
  acceleration, and the push takes the deck from about 1 to 5.7 m/s in one tick. #2's respawn is 14 ticks before its
  ollie at node 58: the action graph takes it (`OllieAnticipation` -> `TrickFromSquare`), the motion graph stays in
  `Bumped`, so nothing pops.
- **Fix (outcome-matched, 2026-10-17):** retail's code also sets the push as one velocity step without touching the
  sampler's history (b194 [code]: `82C04168`, `82C07D20`), but retail's spawn-placed skaters ride off and do their
  tricks. Here the spawn push (`GamePhysics::push_context_velocity`) also gives the acceleration sampler the pushed
  velocity as its previous value, so the push reads as no acceleration; real hits afterwards still read as bumps.
  Muted DownTown run, same seed: all 35 sim ollies / flips pop (was 33 of 35), andrew_reynolds passes node 60, ticks
  in `Bumped` 239 -> 74. Bails 7 in 150 s (6 before; the sessions diverge). Not play-tested.
- **Why `Bumped` lasts the whole clip (b195, b196 [code]):** retail (`82C14810` / `82C14978`) and ours
  (`selection.rs:126-154`) only take a transition whose target has an enabled, active leaf; every active leaf under
  `Riding.Turning.Push` needs a `RightPush` / `LeftPush` intent, which an AI skater does not hold, so `Bumped` runs to
  its `WillExpire 0.1` exits in both. A pushing player leaves at once. Retail behaviour; no change.

## Simulated NPC skaters steer by the effective deck after a flip (2026-10-17)

- **Problem:** muted DownTown runs had NPC skaters stall riding away from their line, tail first (#8 lost 38 m off its
  line after an inward heelflip, #7 likewise).
- **Retail [code] (research b199, b200):** the AI's record, steer and facing gates read the deck forward from PhysOut
  +64, filled by `GetEffectiveTransform` (`82C01BF8`): X and Z negated while the board is flipped under the rider
  (Processed+2468 bit 20). Our sim read the raw deck part, so after a flip trick it steered the wrong end at the line.
- **Change:** `npc_sim.rs` takes the effective forward (the raw deck's forward, negated while that bit is set) for the
  AI record, the anticipation facing gate and the ground steer.
- **Result (same muted run before / after):** NPC_SKATER_LOST 34 -> 9, bails 7 -> 2, NPC_SKATER_BACKWARDS 74 -> 47; the
  4 remaining final-stage stalls are now at small positive speeds (none rolling away tail first). skate-game
  living_world tests 95 pass. Not play-tested.
- **Open (b201):** a skater that really rolls back down a ramp without a flip keeps riding_fakie false because the
  rule reads the animated root, which faces travel; which side is wrong needs one rerun with `flipped`, `root_yaw`,
  `eff_yaw`, `cat` and the fakie clocks on NPC_SKATER_STEER (research b201).

## The stage-2 re-snap is a full path reset (2026-10-17)

- **Retail [code] (research b207):** the stuck re-snap `8246D168` picks
  the node, saves the stuck tracker block (pc+0x210..0x244), runs the path reset `82468AC8` (projection objects,
  search, trick fields, the per-line bytes) and restores the tracker. So it keeps the stage but starts the line state
  fresh, like a line switch.
- **Ours before:** the line switch cleared the air, trick slot and board yaw and reset the tracker (matches); the
  re-snap only moved the cursor (`switch_to`), so a stale air / trick slot / board yaw survived the jump.
- **Change:** `npc_sim::clear_line_state` (air, slot, board yaw) is shared by both; the re-snap calls it before
  `switch_to` and leaves the tracker alone. skate-game tests pass (777). Not play-tested.
- **Also from b207 (no change):** walk-back with no plan and the missing push already give retail's outcome (stage 3
  retries the swap every 60 ticks, stage 4 waits, 5 bails, 6 respawns; the only push is the spawn push). Open: why
  our NPC plans fail (log the solve result on the no-plan line first); whether ours models retail's 896 / 900 clocks
  (reset to 7200 ticks by `82468AC8`).

## The walk-back goal snaps to the nearest reachable point (2026-10-18)
- **Problem:** in the muted stall run stall-7, simulated skater #5 (cuz) stalled at node 152 and the walk-back logged
  "no plan to node 159 / 157" twice. The path service located the goal with the peds' point query (a polygon under the
  point within 4.0 m, else an edge within 0.5 m), and the goal sat 7.86 m below the only polygon above it, 2.67 m from
  the nearest edge, so no search started (`NavMesh::solve`, `locate(to)` returned `None`). Research
  research b276.
- **Retail** (b277, [code] Ghidra + recomp): the path query `82928990` -> `82928770` finds the goal with `82933E18` ->
  `82933A48`: the best distance starts at `0x822F9310` (~9.2e18) and the spatial query range is `0x8206D108`
  (FLT_MAX), so it is unbounded; only polygons in the start's region set count (`8292D250`, goal test `82938C48`, no
  area test); the closest point of each polygon is `8292F9F0`; one result, the nearest. `82928618` ends the path at the
  snapped point. The plan acceptance (`82467088`, 2.0 h / 1.0 v from the last waypoint to the node) then rejects a far
  snap. Not decoded: the start-snap tolerance `*0x83084138` x 0.1.
- **Change:** `NavMesh::locate_nearest(p, from, max_dist)` (3D nearest point over the polygons reachable from the
  start; inside a polygon's outline the surface under the point [ours: vertical, retail projects onto the plane]; ties
  to the lower polygon index). `PathService::solve` takes the goal radius; the navmesh service uses
  `locate_nearest` for the goal, the start keeps the peds' query. New setting
  `ControllerBSettings::goal_locate_radius` (default `f32::MAX` = retail; a mod can cap it). The peds' `NavRules`
  (0.5 / 4.0) are unchanged.
- **Outcome for #5:** the same as before (the 2.67 m snap fails the 2.0 m acceptance, the stuck stages continue, as
  retail). Goals 0.5 to 2.0 m off the mesh now plan instead of failing.
- **Files:** `crates/skate-core/src/living_world/peds/nav.rs` (`locate_nearest`, `closest_on_segment_3d`),
  `crates/skate-core/src/living_world/controller_b.rs` (setting, trait, navmesh service).
- **Tests:** `path_goal_snaps_to_the_nearest_reachable_point` (1.2 m off: snapped and planned; cap below the distance:
  no goal; other component ignored; under the mesh: straight up; 2.67 m off: plan rejected).
- **Open:** the root of the #5 stall is earlier: the nollie at node 146 was dropped because the skater was off the
  path (`npc_sim.rs` trick drop), so it never popped onto the raised section at nodes 152 to 159 (a ledge or manual
  pad, [inference]). Ported in the next section.

## Recorded tricks go out off the path too; leaving the path clears the pending slot (2026-10-18)
- **Problem:** stall #5 (cuz) was 1.22 m off its path at node 142, so our sim dropped the recorded nollie at node 146
  (`NPC_SKATER_SIM_TRICK ... dropped ... off the path`) and the skater never popped onto the raised section at nodes
  152 to 159. Our off-path drop and the frozen slot arm dated from b109 (tricks sent to a sim whose cursor ran ahead of
  it); the cursor follows the skater since b110.
- **Retail** (b279, [code]): the per-tick controller `8246FA30` calls
  the dispatcher `8246A2E0` before the path drop `8246DB50` (recomp order checked), and the dispatcher never reads the
  on-path byte `pc+920`; it is gated by off the board (+0xA1), board toggling / a pending toggle press, and skitching.
  `8246DB50` clears `pc+920` and the pending trick category `pc+832` in the same block on the on-to-off transition
  (stores checked in the recomp).
- **Change** (`crates/skate-game/src/living_world/npc_sim.rs`): only off the board drops the trick dispatch; the slot
  arm runs off the path too; on the tick the skater leaves the path, `sim.slot.category` (our `pc+832`) is cleared
  once, after that tick's dispatch. The drop / commit distances stay the existing `PathTuning` values (mod data).
- **Measured** (muted stall run, DownTown 150 s, deterministic per build: two runs of one build were identical):
  HEAD before: LOST 9, STUCK 161, BAIL 1, off-path trick drops 12; goal search only: LOST 9, STUCK 152, BAIL 2;
  goal search + this rule: LOST 4, STUCK 109, BAIL 6. Of the six bails: #5 now reaches the stuck chain's stage 5 bail
  (retail's recovery, before it never got there), #3 is the same as before, #6 on the path, #2 (twice) and #8 bail
  after tricks fired 1 to 6 m off the path (landings our sim misses; retail fires the same tricks). Watch in play.
- **Tests:** skate-game 806 pass.

## Walk-back hand-back: the line reset clears the air and trick state (2026-10-19)

- **Problem:** stall run stall-9 (DownTown 150 s): NPC #2 was handed back from its walk-back at line node 48, popped a
  nollie at node 56 one tick after leaving its path, and bailed; the STUCK lines show negative signed speed (-3.1,
  -6.0, -9.6 m/s) while the node index rises (research b290).
- **Retail [code, research b291, Ghidra of `824689C0` / `82468AC8`]:** the
  hand-back sets no facing, heading or stance; controller B lines the skater up before it reports "Arrived" (node
  step direction within pi/8, `82467C90` / `82467E48`; ours `controller_b.rs:397-400`), and the re-board is the
  player's get-on. A's activate runs the path reset `82468AC8`, which also zeroes the takeoff flip latch (`pc+927` /
  `+928`), the trick slot (`+832`, `+844 = -1`, `+904`) and the board yaw fields (`+856`, `+860`, `+930`, `+931`);
  only `pc+528..580` is saved and restored around it.
- **Change:** our hand-back now calls `clear_line_state` (the same reset as a line switch and the stage-2 re-snap),
  so a walk-back no longer carries the old line's air and trick state. No facing rule (retail has none).
- **Note:** `NPC_SKATER_BACKWARDS` measures the recording (`replay.cursor.sample`), not the simulated body; node 48 is
  recorded fakie. The body's backwards travel is in the signed speed of the STUCK lines.
- **Verification:** skate-game suite. Not play-tested; the next stall run shows #2 at node 48 / 56.
- **Open:** why our deck can point backwards after the re-board (get-on direction, not read); `B+112` (`82467718`).

## The path projection reads the effective deck (2026-10-19)

- **Problem:** the STUCK lines' negative signed speed (NPC #2 in stall-9, before and after its walk-back, and after
  an earlier respawn) could mean real fakie travel or a rider standing backwards on its board.
- **Root cause [code, research b292, verified]:** on board,
  `projected_point` (`npc_sim.rs`) returned the raw deck column 2, while retail's path projection `sub_8246BCF8` reads
  the solved effective deck (`sub_82592A00`, `GetEffectiveTransform` 82C01BF8: columns 0 and 2 negated while
  Processed+2468 bit 20 is set; ours `board_motion_output.rs:31-34`). Our steer already used the effective forward.
  The signed speed feeds the projection's tick and the stuck / lost checks.
- **Change:** one helper `effective_deck_forward` for the steer and the projection. No retail facing rule at the
  re-board (b291 / b292: none exists; the board keeps its yaw, the stance comes from the mount clip's
  `animboardbackward` event).
- **Verification:** skate-game suite (811). Muted stall run stall-11 on 294af680 (150 s, DownTown, 3 NPC physics
  loads as stall-9): lost 2 (stall-9: 4), stuck 54 (109), bails 3 (6, all WipeoutGround), BACKWARDS 23 (62), no panics.
- **Open:** which mount clip sets `animboardbackward` for a hand-carried board; the off-board heading (`[Skeleton+0]`
  vs our `root[2]`, open since b116).

## The record's trajectory comes from the cursor's node only (2026-10-19)

- **Problem:** NPC skaters #2 and #8 locked onto grinds that have no recording (b293). The grind gate (`82D68FE8`,
  `scoring.rs`) reads record bits 26 / 27, and our record set them from the NEXT recorded jump, however many ground
  nodes ahead.
- **Retail [code]:** `sub_8246DA18` fills the trajectory only from the PathController's current ext (`pc+692`, ext
  byte `+0x27` bit 0 HasTrajectory) and takes bit 27 from `sub_824551C0(path, max(pc+704 - 1, 0))` (checked both
  in the recomp's `8246DA18`). Which node `pc+692` is: the cursor's node while the segment fraction is below 0.5,
  else the next one, the last node its own (b294, from the cursor writer; the 0.5 constant rests on b294's reading,
  not re-read). Between jumps the record has no trajectory and the gate reads the cached previous jump
  (`publication.rs`), which is retail too (b294 section 2).
- **Change:** `ai_record::trajectory_node` / `current_trajectory` replace `upcoming_trajectory`; `npc_sim.rs` passes the
  cursor's node and `frame_in_segment`, and `lands_in_grind` starts at the cursor's node - 1.
- **Log:** `NPC_GRIND_ENTRY` on every NPC grind or slide entry (`player_state/transition.rs`): the gate's bits 26 / 27,
  `flags_2472` bit 29, this tick's record bits and `fresh`, the processed-input cache's bit 27, and the cursor (node,
  frame in segment, ext node, whether it had a trajectory, the node `lands_in_grind` started from). The trajectory
  selector's candidate index (b294 asked for it) is not logged yet.
- **Mod reach:** the window and bit 27 come only from the line's nodes and ext flags, so a mod's line drives them.
- **Tests:** `a_recorded_trajectory_fills_the_record_block` (nearest node, the half-segment switch, none between
  jumps, the last node).
- **Open:** our cursor is time-based, retail's `pc+660` follows a projection (`8246E3B8`).

## Teleport reset: the grind manager, the trajectory selector and the landing deck; a fresh cache for a new actor (2026-10-19)

- **Problem:** a grind lock (engagement counter past 151, then a 90-tick cooldown, `grind/history.rs`) and the
  selector's grind-locked-to-middle bit survived every teleport, respawn and placement, and a pooled NPC body handed
  the previous NPC's processed-input cache (with a stale "lands in grind" bit 27) to the next NPC. Both are candidates
  for the grind stalls of #2 / #8 (not measured yet; the `NPC_GRIND_ENTRY` log shows it).
- **Retail [code]:** ResetSystems `82DB92D0` (the teleport / placement reset) makes 14 calls (b297 table). Ours had
  10 of them; missing were TrajectorySelector `82D67228` (ours cleared only `trajectory_available`), OffBoardFeetIK
  `82D77168` (b298: two foot records and the manager's tail back to their initial values, no child objects),
  LandingOnDeck `82D78B28` and, last, the grind manager `82D8A638` (checked `lwz r3,1872(r30); bl 82D8A638` at the end of
  `82DB92D0`, and `+484` = the 90-tick cooldown in `82D8ABD8`). `82D8A638` leaves the manager's child objects
  (balance, jumper, history) alone, and `82D67228` keeps the in-flight trajectory request. A new NPC is a new actor:
  `82590DC0` allocates a PhysicalPlayer and runs `82DB18A0`, so its cache at `+1008` starts empty; the same actor
  keeps its cache across a respawn or its next line (b295).
- **Change:** the teleport block in `input_phase.rs` (taken when a reset is requested, before the input phase and
  the grind gate) now calls `trajectory.selector.reset()`, `offboard_feet.reset()`, `landing_deck.reset()` and
  `player_input.grind.reset()` (no longer test-only), in retail's order. `spawn_sim` gives a pooled body `reset_external_physics_cache()` (82DB18A0's value, shared
  with `initial.rs`) and drops the last owner's AI record. Same path for the player, ambient NPCs and mod-spawned
  NPCs, no dependence on pool order.
- **Tests:** `a_teleport_resets_the_grind_lock_and_a_new_actor_gets_a_fresh_input_cache` (data-gated: the lock is
  cleared by a teleport, the actor's cache survives it, the new-actor reset restores the 82DB18A0 cache).
- **Open:** the two float constants of `82D77168` (0x82165A10, 0x8231A844) are taken as 0.0 / 1.0, not read from
  the image; `physics/offboard/feet.rs` and its siblings are uncompiled duplicates (not in `offboard/mod.rs`); the
  rest of a pooled body (takedown latch, timers, the grind orientation random stream, b296) still carries over.

### Follow-up: a pooled body resets like a fresh one (2026-10-19)

- **Problem:** the rest of a pooled body (above) carried over to the next NPC: grind input and the takedown latch,
  post-input counters, the trajectory selector's pending batch, and the grind orientation random stream at the last
  owner's position. So what a new NPC did in its first ticks depended on which body the pool handed it.
- **Retail:** a new actor gets a new PhysicalPlayer (`82590DC0` -> `82DB18A0`, b295), so every ctor-built part
  starts fresh. Our loaders are the ports of those ctors, so the reset re-runs them (b299).
- **Change:** `SkaterRuntime::reset_for_new_actor` (replaces `reset_external_physics_cache`) re-runs
  `PlayerInputRuntime::load` from the collections already in memory (`GamePhysics::collections()`, no file I/O). It
  keeps only the physical / processed input words, which mirror the live state and get the placement's state Exit /
  Enter. It also resets the post-input state, the trajectory selector, its pending batch and nearby grinds, the grind
  orientation stream (fixed seed `821642B0`, same as every fresh body), the chromosome, the pending wipeout impulse and
  the grind manager, `takedown`, `ai_physics` and the skitch-release trace. Only the pool arm of `spawn_sim` calls it;
  a respawn or next line of the same actor keeps everything, as in retail. Mod-spawned NPCs take the same path, and
  mod content overrides reach a reused body through the same loaders.
- **Tests:** `a_teleport_resets_the_grind_lock_and_a_new_actor_gets_a_fresh_input_cache` now also dirties the grind
  lock, post-input counter and skitch trace and checks that the new-actor reset restores them.
- **Open (b299 §5):** whether keeping the processed / physical words matches a fresh body after the placement tick,
  and the subsystems not audited field by field. A pool-order test (same NPC from a dirtied and a clean body, compare
  60 ticks) would show any remaining carry-over.

## Simulated NPC skaters at every distance (2026-10-19)

Problem (PR #52 next item 3): retail simulates every ambient skater; ours switched an NPC from replay to physics only
within 40 m of an observer and handed it back beyond 44 m.

Retail [code, research b303; verified skater vtable `0x82300910`
slot 0x38 = `8258E8B8`]: the SimController update (`sub_82859E70`) runs its fixed phases over the whole actor list with
only an update-mask filter (`actor->vtable[0x38]() & mask`); the mask is fixed at creation (`skater+1772`), 1 for every
normal skater, player or AI, so every skater gets the full update every tick at any distance. No distance or count
tier, no lower rate. The limits are the population's (3 desired offline, AI cap 5, cull beyond 120 m), already ported
(`skate-core/src/living_world/skaters.rs`).

Change: `npc_simulated.radius` defaults to `f32::MAX` (`npc_sim.rs`), so the hand-back (`distance > 1.1 x radius`) and
the "too far" wait never fire; a new skater starts on node 0 and switches to physics on its first safe node, as
retail places an AI skater on a node with the spawn push. A finite radius (mod key, 0 to 500 m) stays a mod's cost
choice; `max` stays 3. The tier itself stays opt-in (`SKATE_NPC_SIM=1` / the living world NPC physics choice) until
play-tested. Cost: the per-NPC solve dominates (a local research note); speed-ups only behaviour-identical.

Test: `living_world_npc_sim_has_no_default_distance_limit` (npc_tests). skate-game unit tests 814 passed. Not
play-tested; frame time with three far skaters simulated not measured yet.
Open: whether anything inside retail's per-skater physics thins out far skaters (below `sub_82594B48`, not read).

## Riding tail first: the fakie rule's input lines up with the deck (2026-10-19)

Question (PR #52 next item 5, b199 / b200 / b201): NPC skaters sometimes stalled riding tail first; b201 found the
fakie rule's input (effective animation root Z) not lined up with the deck and listed debug fields to decide which side
was wrong. Retail's writer of the riding-fakie byte is known (b200, `sub_82DB6EC0`).
Change (diagnostics only): `NPC_SKATER_STEER` (debug, `SKATE_NPC_TRACE_TICKS`) now also logs `flipped` (the packet's
board-flipped bit), `root_yaw` (yaw of the effective root Z, the rule's input), `eff_yaw` (the context's effective
board forward, `GamePhysics::context_effective_forward`) and `cat` (filtered category).
Run [trace, ours]: muted `npcsim.sh` on DownTown, 150 s, radius 150 m, 11 957 steer lines, 0 panics, 3
bails. Of the moving samples (speed >= 1 m/s) with the deck backwards to the velocity, root and effective forward point
with the deck in every one (none of b201's mismatch rows); on the ground 421 report fakie true and 11 false (short
moments after a revert or near 1 m/s, the rule's slow-backwards wait); grinds keep the previous value as the rule does.
Of 22 stall starts (`NPC_SKATER_STUCK` stage 0 -> 1), 3 were tail first, all at 0.3 to 1.1 m/s with fakie still
false; 19 were riding forward at 2 to 5 m/s.
Result: no fakie change needed; b201's mismatch no longer shows (fixed by the later effective-deck work, "Simulated NPC
skaters steer by the effective deck after a flip"). Open: the forward stalls at 2 to 5 m/s (most stall starts in this
run), and whether retail's slow-backwards wait matches ours at 0.3 to 1.1 m/s.

## A spawn push does not read as Bumped, as retail (recomp check, 2026-10-19)

Question (PR #52 next item 4, b194 O1 / b195 / b196): after its spawn push, does a retail AI skater read as Bumped
(`IsBumped` `82BA7310`), which would keep it from tricking? Ours avoids the spike by pre-filling the observed-velocity
history with the pushed value (`GamePhysics::push_context_velocity`, since 505dc2ef); retail's push `82C04168` does not
touch the history (b194 [code]), so ours is our own mechanism and needed the retail outcome to judge it.
Trace [trace, the recomp]: new research hooks (`hooks_physics.cpp`, category `aiskater`): `AIPUSH` (the push when
called from the AI hold branch `824701F8`) and `BUMPQ` (every true IsBumped result, every evaluation within 1 s after
an AI push). Background run, player idle 120 s at the save's spawn, `SKATE3_TRACE=aiskater`: 2 AI pushes (3.6 and
18 m/s), 157 IsBumped evaluations on 5 actors within 1 s after them, all false; the 6 true results all came at other
times. Malformed lines 0. (The push logs the board, not the actor, so the pushed skater is not tied to one actor id;
every actor evaluated in the window read false.)
Result: retail does not read Bumped after the spawn push, which is the outcome our pre-fill gives; kept, now pinned by
`a_spawn_push_with_a_prefilled_history_does_not_read_as_bumped` (skate-core `physics/tests/board_ground.rs`). No
engine behaviour change. The hooks go to the recomp fork's research-hooks branch in the commit window.

## NPC skater hitches of ~200 ms: a failed first step dropped a pooled body (2026-10-19)

Problem (laptop sessions 2026-10-19 10:08 / 10:16, hails-local, NPC physics on; the user reported a large hitch near the
beginning): three FRAME_HITCH of 203 to 213 ms, all in npc_sim (normal 0.5 to 3.8 ms).
Cause [trace + code]: each hitch is an `NPC_SKATER_SIM_LOAD` of 192 to 201 ms on the game thread, right after
`NPC_SKATER_SIM #N: physics error, back to replay: KnownAir requires a completed selected trajectory`, which came on
the first physics step after a handover (one tick after `<- replay`). Two faults (research b310, verified):
1. The error path removed the `NpcSim` component, dropping the pooled body, so the next switch loaded a new one
   synchronously (`npc_sim.rs`, the step results loop).
2. A body pooled mid-jump keeps KnownAir; the new-actor reset (b299) replaced its trajectory selector at once, and the
   placement's state Exit then ran KnownAir's Exit (`known_air.rs` `frame()`), which reads the selection.
Retail builds a new actor per NPC (`82590DC0` -> `82DB18A0`), so no old state ever exits on it.
Change: (1) a failed body goes back to the pool like a hand-back (`pool_from`); (2) `reset_for_new_actor` marks
`trajectory_reset_pending` and the placement resets the selector right after `enter_after_teleport` (`frame.rs`), so
the old state exits with its selection and the new actor still never completes the old batch (b299's goal).
Test: `a_teleport_resets_the_grind_lock_and_a_new_actor_gets_a_fresh_input_cache` (data-gated) checks the pending
reset and that the next placement does it. Not play-tested.
Checked 2026-10-19 (b362) [code]: KnownAir is the only state whose
Exit reads the selector (covered by the deferral); every other Exit dispatched in `transition.rs` and both placement
Enters (PhysicsGround, BipedGround) leave it alone (main spot-checked PhysicsAir's Exit). The "grind trajectory
target" b310 named is KnownAir's Enter / Update (`start_grind_air_adjust_from_selector`), not an Exit. Retail's
teleport resets its selector (`82D67228` via ResetSystems `82DB92D0`) before the old Exit and the Ground Enter
(`82DB8540(100)`), the same order as ours. Open: whether a pooled body's first step is always the placement (else one
Update would read the old, complete selector: a stale aim, not an error); a snapshot of `SkaterRuntime` must carry
`trajectory_reset_pending`.

## A walk-back that frees a stuck skater says so in the log (2026-10-19)

Laptop session 2026-10-19 11:05 (the user: near the end an NPC skater got stuck in a corner on the bridge over Rosalita
skate park): #15 logged stage 1 -> 2 -> 3 at node 45 and then nothing. Research b319 (verified the log and
`ai_controller.rs`): the stage 3 walk-back to node 64 worked (hand back 17:12:52.18, rejoined the line, trick at node
69); `reposition_done` clears the stuck watch without a line, so the recovery was invisible. The re-snap 45 -> 45 is
retail's rule (`8246D168`: skip ahead only where the line climbs steeper than 1.0; else the nearest node). Change:
`NPC_SKATER_STUCK #id name stage n -> 0 after walk-back to node m` at the hand-back (`npc_sim.rs`); the re-snap
budget (240 summed node frames) is `ControllerSettings::resnap_frames`. Session counts: 1 -> 2 32, 2 -> 3 19,
3 -> 4 4 (each after a "no plan" walk-back failure), 4 -> 5 (bail) 3, stage 6 none. Open: why the navmesh walk-back
plans fail at the same spots (nodes 69 and 36).

## The walk-back start snaps from an obstacle onto the mesh (2026-10-19)
- **Problem:** laptop session 2026-10-19 11:05 (`game-20261019-110510`): six stage-3 walk-backs logged "no plan" at the
  same spots (nodes 36 x2, 69 x2, 29, 23), so those skaters climbed to stage 4 / 5 (bail). Every start was off the
  peds' navmesh by more than their 0.5 m snap: inside the hole the navmesh cuts around an obstacle (0.65 to 2.64 m from
  the nearest walkable edge), two of them 2 to 3 m up on top of a grind block (#7 in `GrindFiveO` at y 34.3 just
  before). `NavMesh::solve` stopped at `locate(from)` before looking at the goal. The goals were fine (inside or 0.34 m
  from main-component polygons). Research b324 (offline check of the
  logged points against the session's DownTown navmesh).
- **Retail** ([code], Ghidra, b324; constants as b277): the path request `82928770` finds the start polygon with
  `82933848` -> `829338B0`, the same unbounded nearest-polygon search as the goal (query range `0x8206D108` FLT_MAX,
  best distance from `0x822F9310`), closest point `8292F9F0`, 3D distance. Replaying it offline, all six starts snap
  0.67 to 3.61 m onto main-component polygons and every plan passes the 2.0 h / 1.0 v acceptance, so retail starts
  these walk-backs.
- **Change:** `NavMesh::locate_start_nearest(p, max_dist)`: the nearest walkable polygon point at any distance, any
  component (inside the outline the surface under the point, else the nearest edge point), a ring search over the
  grid that stops once no farther ring can be nearer; ties to the lower polygon index. The navmesh path service uses
  `locate(from)` first (unchanged on the mesh) and this search when it fails. New setting
  `ControllerBSettings::start_locate_radius` (default `f32::MAX` = retail; a mod can cap it, 0.5 = the old
  behaviour); `PathService::solve` takes it. The peds' `NavRules` (0.5 / 4.0) are unchanged.
- **Files:** `crates/skate-core/src/living_world/peds/nav.rs` (`locate_start_nearest`, `closest_point`),
  `crates/skate-core/src/living_world/controller_b.rs` (setting, trait, navmesh service).
- **Tests:** `walk_back_start_snaps_from_an_obstacle_cut` (a start 2 m inside a 4 x 4 m cut and 2.8 m up snaps to the
  cut's edge; a 3 m cap refuses it; the ring search equals a scan of every polygon at six points; the plan is accepted,
  and refused with the old 0.5 m).
- **Open:** whether retail's candidate filter `82938A90` for this query rejects the same areas as our `walkable`
  (area 0xF1) [unverified]; retail keeps the raw start as path element 0 (`82928530`, b277), ours aims at the snapped
  start first (the same walk off the obstacle); a start high on a wall-top island snaps to that island, as retail.
  The controller settings are mod-reachable now ("The NPC skater controllers are mod-reachable" below).

## The NPC skater controllers are mod-reachable (2026-10-19)
- **Why:** ask 80 follow-up and the moddability rule: only `npc_simulated.walk_arrive_distance` / `walk_stuck_ticks`
  reached a mod; the walk-back controller (controller B) and the line controller (stuck tracker, re-snap, reposition,
  safe node) kept their retail values out of reach. Research b330.
- **Change:** two `living_world` world-tuning keys, the same pattern as `npc_avoid`:
  `npc_walk_back {...}` (every `ControllerBSettings` field by its Rust name, e.g. `start_locate_radius`,
  `goal_locate_radius`, `arrive_h`, `stuck_waypoint_ticks`) and `npc_line_controller {...}` (every
  `ControllerSettings` field, e.g. `resnap_frames`, `stuck_limits` (six), `safe_run`). Retail values stay the
  defaults; floats >= 0, counts <= 36000 (ours), `push_period` and `safe_run` >= 1. Applied after `npc_simulated`, so a
  nested key wins over its alias. Read back in the `living_world` read. Patches are per mod; a mod that stops loses
  them (`clear_owner`), the first writer keeps a field another mod also sets. Host-side (the NPC simulation runs on
  the host); the stored patches are JSON.
- **Files:** `crates/skate-mods/src/world_tuning.rs` (`NpcWalkBackPatch`, `NpcLineControllerPatch`, macro
  `settings_patch!`), `crates/skate-game/src/modding/world_tuning.rs` (apply, read), `sdk/skate.lua` (stub list).
- **Tests:** `npc_controller_settings_are_mod_reachable_and_reset_on_disable` (retail defaults, partial patch,
  nested key over the alias, first writer per field, rejected values, reset on disable).
## NPC skaters speeding up uphill: the board path, and a bail speed that misread (2026-10-19)
- **Report:** laptop 2026-10-19 16:11 (University): an NPC skater "went uphill ... without pumping, he randomly just
  gained a bunch of speed for no reason", and four skaters on one line bailed at "40 m/s" (`NPC_SKATER_BAIL`).
- **Cause** ([code] + [log], research b338): the AI board path (`82C05EC0`,
  `skate_core::riding::grounded::state::board_path::update_board_path`) pulls the deck velocity toward the record's
  target velocity (the recorded node step x 60) by up to 0.2 m/s per tick, whatever the slope; the University library
  lines are recorded fast (about 14-20 m/s), the spawn push starts at 0.75 of that. Both are retail; not changed.
  The 40 m/s bails were our measurement: the speed was the projected point's step x 60, and on the wipeout tick the
  point switches from the deck to the character root.
- **Change:** the bail line's speed is the deck body's velocity; a new `NPC_SKATER_SPEED` line (the deck speed changed
  by 2 m/s within 30 ticks; recorded speed, slope along the motion, state, node, on_path, hold) so a speed-up shows its
  source.
- **Open:** whether a retail AI skater plays a push while the board path speeds it up (who posts RightPush for AI
  skaters: Ghidra or a short recomp trace at the library spawn); the board path's share of each change in the line.

## AI skaters push when slower than their line (2026-10-19)

**Problem.** Laptop session 2026-10-19 16:11 (USER-ASKS 112): an NPC skater sped up uphill without pushing. b338
found the speed-up is retail's board path pulling toward the recorded line speed; the open question was whether
retail also plays a push there.

**Retail (research b341; verified the four intent keys in `sub_8246B4C8` and the push writer and caller as
Ghidra pseudocode) [code].** The PathController's board tick `sub_8246BAA8` (called from `sub_8246FA30` on the
ground branch, not in the air) computes the target speed (the recorded step x 60, floored at 4.0 m/s when more than
2.0 m off the line, then the avoider's shaping `sub_82470830`). Below it the push writer `sub_8246B4C8` posts
`RightPush` or `LeftPush`, `NewPush` and `Pushing`, all 1.0, into the same intent map the pad listener fills, so the
graph plays the player's push. Gates: no push in trick categories 4 / 7 / 10 / 12, at stuck stage 4, when the line
points against the board's forward, or with a heading error of 40 degrees or more. The factor is 0.97 of the target
(0.3 when a start-trick node is under 40 recorded frames ahead and the skater is above 2 m/s), times 0.3 each for an
off-board node under 60 frames ahead, a slope under -0.22, and a speed above 11 m/s. No timer: it re-posts every tick
while the condition holds. So retail glides on fast lines (the b338 University case, 14 m/s against a 19 m/s line,
posts no push in retail either) and pushes on slow ones, where ours posted nothing.

**Change.** `skate-core` `living_world/ai_push.rs`: `PushSettings` (retail defaults), `target_speed`, `slope`,
`frames_to` / `trick_frames` / `off_board_frames` and the pure `push` decision. `npc_sim.rs` posts its intents after
the ground steer on ground ticks on the board, outside a hold, a respawn wait and skitching; the debug line
`NPC_SKATER_AI_PUSH` names where a push run starts. Mods: `npc_simulated.push`, `push_factor`, `push_reduce`,
`push_fast_speed`, `push_downhill_slope`, `push_max_heading_deg`, `push_right_foot`.

**Foot and brake (b347; verified both getters in the recomp code) [code].** The foot: `RightPush` when the
SkaterAnim component's (`0x82453410`, "ISkaterAnim") stance-mirror bit (slot +0x1C `82B97140`, flags bit 30) differs
from its riding-fakie bit (slot +0x0C `82B970D8`, bit 29): regular forward pushes right, goofy forward left, fakie
flips it; ours reads the same two bits (`SkaterAnimation::stance()`), `push_right_foot` is only a mod override now.
At or above the target the brake writer `sub_8246B810` posts `Brake` (`ManualBrake` in a manual, category 8) only
when speed x 0.3 (0.12 with an off-board node ahead; x 0.4 uphill above 0.22) beats the target, the line cursor is
past its first node and the heading error is under 40 degrees, with a magnitude ramped to 1 over 0.1 m/s; between
the target and that it glides (`ai_push::brake`, `BrakeSettings`; mods `npc_simulated.brake`, `brake_factor`,
`brake_ramp`). Tests `the_foot_follows_stance_and_fakie_like_retail`, `brakes_only_far_above_the_target_with_a_ramp`.

**Not retail yet / open.** The slope uses the recorded step as the second point (retail passes the recorded
position and a second point; inferred). The 0.99 factor of `sub_824681D8() == 23` (mode not decoded) is not ported.
Whether the graph's push adds speed on top of the board path for an AI skater as for the player is expected (shared
graph) but not traced.

**Tests.** `ai_push` (skate-core): `pushes_below_the_factor_and_glides_near_the_target`,
`fast_lines_glide_up_to_the_recorded_speed`, `gates_and_reductions_follow_retail`,
`target_speed_floors_far_off_the_line_and_slope_is_rise_over_run`; skate-mods patch validation for the push keys.
Not play-tested yet.

## Code reference: living_world/npc_sim.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/npc_sim.rs`; each comment there links its section here.

### Simulated NPC skaters model

Simulated NPC skaters (doc 26, "Simulated NPC skaters", M7): an NPC skater is a full physics skater (its own board context and `SkaterRuntime` in the shared world) driven by its AI record, taken over from the replay tier on its first safe node.

Retail runs every ambient NPC skater simulated at any distance (at most 3 offline; b303: the SimController updates every skater each tick, no distance or count tier). So the radius defaults to unlimited; a finite `npc_simulated.radius` is a mod's cost choice (handed back beyond 1.1 x, never switched while airborne or grinding); the handover keeps the line's speed (retail never hands over). Off by default until play-tested: `SKATE_NPC_SIM=1` or `sdk.world.set_tuning("living_world", {npc_simulated = {...}})`.

Per tick (after the population and the replay cursors advanced): the record is built from the cursor (`skate_core::living_world::ai_record`), the retail spawn push applies while the skater is still on its node, and `GamePhysics::advance_npc_skater` runs the same frame as the player. The NPC is drawn from its simulated pose (`render_pose`, like the player); its population position and audio still follow the cursor. A physics error drops it back to the replay tier.

### Simulated backwards line

The `NPC_SKATER_BACKWARDS` line for a simulated NPC is drawn from its own body, not the replay cursor's ghost (b352: every ghost line in one session was a sim NPC), so it reads the body: effective board forward more than 135 deg from the deck velocity at 1 m/s or more (the replay check's diagnostic threshold), with the body's retail fakie bit (`82BB2330`; a fakie rider is not wrong).

### Effective root and B reads

What B reads from the simulated skater (b80): `[rec+56]+161` = SkaterOffBoard (Processed +2484 0x08000000), `+160` = TransitioningOnOffBoard (+2480 0x4), both written by `82DB6EC0`; "can move" (`82466ED0`) = off the board and not toggling (its `[rec+72]+308 / +309` terms are not identified: taken as clear); the hand-back byte `[rec+28]+59` = the wipeout bit (`82DB6EC0` copies it there).

The skater's effective animation root (`GetEffectiveRoot` `82BE3650`): rows 0 and 2 negated while Processed 2476 bit 2 (the mirrored stance) is set. The raw root of a goofy / mirrored skater faces backwards: controller B then walked the skater straight away from its node.

### Clear line state

`sub_82467FC8` (b116, verified the state 500 / wipeout byte tests): the point the path projection, the lost test and the speed (`pc+96`) use, with the heading `pc+128` (`sub_8246BCF8`). Off the board (BipedGround) or wiping out: the animation root and its forward row; on the board: the board (retail: the board rigid body's position, SkateboardReckoning +144; ours the deck part, open) and the deck's forward. The per-line state retail's path reset `sub_82468AC8` clears (air, trick slot, board yaw), shared by a line switch and the stage-2 re-snap so the two cannot drift apart. The tracker block is not touched here: a line switch also resets it (`path_reset`), the re-snap `sub_8246D168` saves and restores it around the reset (b207).

### Controller A activate

A re-attaches by its saved line id at the node (`824689C0`); a success resets the line controller's sub-mode (b66). Retail's A activate (`sub_824689C0`, b115): the controller reset (off the path) with the search at the node nearest the skater over the whole line (`sub_82455108`); the rejoin commit puts it back on the path.

### Activate reset

The activate's reset (`82468AC8`) also clears the takeoff flip latch (`pc+927` / `+928`), the trick slot and the board yaw; none of them is in the saved `pc+528..580` block, so a walk-back no longer carries the old line's air / trick state onto the new one (b291). Retail sets no facing here: controller B lined the skater up before "Arrived".

### Projection and avoider

The obstacle avoider's answer (`npc_avoid`): retail caps / floors the record speed (`sub_82470830`) and moves the target across the path (`sub_82464E30`, controller `+933` = steering). Retail's projection (`sub_8246D560`, b110): cursor A from the skater's position, B one shaped recorded tick ahead; the cursor (and so the target, the record and the trick events) goes to B.

## Code reference: living_world/ai_record.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/ai_record.rs`; each comment there links its section here.

### AI physics record

The NPC skater's AI physics record (AIPhysicsInput, 164 bytes) built from its recorded line (doc 26, "Simulated NPC skaters"). The physics reads it in the ground states' board path (`crate::riding::grounded::state::board_path`).

Retail (TU3, evidence only; re-implemented): `sub_8246DB50` -> `sub_8246DE38` each tick:
- target frame = the path frame of the committed cursor node (`sub_82453AD0`: board orientation `+0x18`, turned about up on board-flipped nodes, `sub_82453A58`) at the node position; when its forward points away from the skater's (`dot < 0`), its Ri and At rows are negated (the line is ridden switch / fakie instead of turning round);
- target velocity = the node's per-frame displacement (`+0x0C`) x 60 (`0x8302EE08`), its length clamped to 99.9 (`0x822F94D4`); above 0.001 (`0x82063A48`) the speed shape `sub_82470830` may scale it (`0.3 * s * influence + 0.125 * s * speed factor`, applied when it changes the speed by more than 0.01; both inputs not decoded yet, 0 here = no change);
- flags `+160`: bit 25 = on-board steering (`pc+922`, seeded by `sub_8246DB50`); with it (`pc+922`) bit 31 always, bit 30 = A and C, bit 29 = B and C, bit 28 = B, where A = (state byte `14652` clear and `pc+933` clear), B = A and `pc+945` clear, C = `pc+944` clear or trick category (`pc+832`) 9; without it all of bits 31..28 (holding the cached pose when the skater is more than 10 m off the path, `0x821963E4`, AI kind 0). The record's other words (trajectory block +80..+144, bits 27..25) are not built yet.

### Spawn push

The spawn push (`sub_824701F8` hold branch): while the skater is still within 0.01 m (`0x820D71E8`; 0.05 m `0x82165A00` when the cursor's node `pc+816` is not 0) of its node in the ground plane, every board part gets the node's per-frame displacement x 60 x 0.75 (`0x821814A0`; x 0.5 `0x8209975C` for AI kind 1 outside motion states 19 / 20) as its velocity (`82C04168`). `None` once it has left the node.

### Lands in grind

`sub_824551C0(path, node)`: whether the recorded jump after `from` lands in a grind or slide. From the first node carrying a trajectory on, it walks the line summing the recorded frames: a start-trick of category 5 (grind / slide, `catalog::category`, `sub_824545F0`) answers yes; category 8 / 9 (manual / powerslide), incidental air, an end-trick on the ground, another trajectory more than 60 frames later or more than 600 frames in all answer no; after an airborne end-trick the first ground node answers yes only when the next node starts a category-5 trick.

## Code reference: living_world/ai_signals.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/ai_signals.rs`; each comment there links its section here.

### AI trick signals

The ActionGraph signals an AI skater's PathController posts so a simulated NPC skater rides into its recorded tricks like a player would (doc 26 "Simulated NPC skaters do their tricks"). Retail writes named signals into the same intent map the player's pad listener fills; no pad bits or stick gestures (a local research note, verified `sub_824691A0` and the signal addresses).

Retail (TU3, evidence only; re-implemented):
- anticipation `sub_824691A0` (each tick from the node advance `sub_8246D560`): within 3.0 m of the line (`pc+828`, `0x82063B08`) it looks up to 60 recorded frames past the cursor for a start-trick node (`sub_82456550`); an ollie or flip there (category 1 / 2) sets `AnticMag` 1.0 and `AnticAngle` 0, or pi for a nollie (the recorded trick, `sub_82469048`); `sub_824696B8` posts them (and `Crouch` on a crouched node outside a trick, `Manual` before a manual) when the skater faces along the line (under pi/4, `0x821A9C28`) [code, the posting gates partly inferred];
- dispatch `sub_8246A2E0`: on the tick the committed cursor reaches the start-trick node it posts `Trick`, the trick's scorable name, `GestureSpeed`, `TrickHeight` and `DontMirrorTrick`, all 1.0 (the graph enters the trick on `HasAGIntent <name>`, names compare case-insensitively); only the newest event node of the tick, none when the cursor crossed more than 3 nodes (1 right after spawn). That perform path is the ollie / flip one only (categories 1 / 2, `sub_8246A080`); the other categories keep the trick in the slot and the per-tick writer posts its timed intents, like a player's held inputs (below, research b113).
- trick slot: `pc+832` category (from the recorded trick, table 0x820862A8), `pc+844` trick, `pc+904` ticks since the start node (0 there, +1 after every writer call); an end node or a failed dispatch clears the category (0x8246A6D4). Jump table 0x8246A45C: 1 / 2 perform; 3 / 4 / 12 take the trick only when the profile's slot-tricks byte (+51) is set (`sub_82469EF0`; else the slot keeps the old trick); 5 / 7 / 10 / 13 take it; 6 / 9 only validate; 8 is the manual value; 11 (revert) posts `Trick` and `SlideFs180` (fsrevert, id 4) / `SlideBs180` (bsrevert, id 5) (`sub_8246A1E8`).
- per-tick writer `sub_824696B8` (jump table 0x824698F8): categories 1-5, 7, 10, 12, 13 post the slot trick's `SkaterIntentTuple` list (`sub_82470ED0`: entry live from `start` to `end`, `end` < 0 = open); 6 (handplant) posts `GrabWorld` 1.0; 8 `Manual`; 9 the powerslide keys; 11 nothing.
- takeoff board yaw (`sub_8246B1F8`) and its post in the air arm (`sub_8246B358` -> `sub_82471008`): see [`BoardYaw`].

### Board yaw reads the landing node

**Problem.** NPC skaters missed about 28% of their recorded grinds in a 23-minute session, and caught grinds were often another family. Of the recorded truck grinds that came out as boardslides, 12 of 12 had a takeoff yaw posted, against 0 of 13 for 50-50s and 1 of 19 for 5-0s.

**Root cause.** `sub_8246A700` on the takeoff edge calls `sub_824566C8(line, pc+652)` from cursor A: the first node without the airborne flag after one with it, not counting a landing while the open trick's category is 13 (start node sets the category, end node clears it). That node goes into cursor C (`pc+712`), which `sub_8246B1F8` reads; with no such node `pc+0x3A2` stays 0 and nothing is posted. Ours read the replay cursor's node at takeoff. The flip latch reads cursor A's frame (`pc+0x260`); ours read the replay cursor too.

**Change.** `ai_signals::landing_node`; `npc_sim` uses it and cursor A for both. Test: `the_board_yaw_node_is_the_first_ground_node_after_the_air`. Diagnostics: `NPC_GRIND_TRY` (each airborne NPC tick near a rail) and `NPC_GRIND_DROP` (a candidate rejected after the investigation); tally tool `tools/dev-bench/npc_grinds.py`.

**Cause found (2026-11-02, research b801).** At DownTown rail 6126 a recorded truck grind became a boardslide: the recomp's skaters did 50-50s and 5-0s (9 of 9), ours boardslid with the board about 90 degrees off the body from a recorded twist of 31 degrees (the recomp's board turns about 26 degrees in the air). The takeoff magnitude scale was wrong: retail multiplies |d| by 2/pi (`0x822F8B7C`), we used 6.0606 (`0x822F8C7C`), so a 31 degree twist posted 3.3 (full `IA_ADJUST_FLAT_FS` clip) instead of 0.345 (about a third of it). Fixed, see [Takeoff board yaw](#takeoff-board-yaw). Open: a re-run of the idle test at rail 6126 to confirm truck grinds, and whether the node 39 bails that start with a board adjust go away. Evidence: research b800, research b801. Idle test runs: `SKATE_TEST_STEP_OFF=<tick>` presses the player's off-board toggle once.

### Takeoff board yaw

`sub_8246B1F8` on the takeoff edge, from the cursor node's recorded board and skater frames: d = heading(board forward) - heading(skater forward) ([`retail_heading`]), wrapped to [-pi, pi], folded to [-pi/2, pi/2] (nose and tail lie on the same board line); outside the dead zone the angle is +-pi/2 by the sign of d and the magnitude |d| x scale (2/pi, not clamped here).

## Code reference: living_world/npc_tricks.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/npc_tricks.rs`; each comment there links its section here.

### NPC trick choice

Which trick an NPC skater does at a recorded trick slot (doc 26, "NPC skater trick choice").

Retail (TU3, evidence only; re-implemented, nothing copied): the PathController's dispatcher `sub_8246A2E0` runs on every start-trick node (event 1) the skater passes. Ollie and flip slots (table820862A8 category 1 / 2) go to `sub_8246A080`; every other category (grinds, grabs, manuals, slides, reverts, plants) does the recorded trick.
- A higher chain level (heelflip2 and up) takes no action: the base trick already running covers it.
- Behaviour mode (behaviour context +60): 0 = the recorded trick (`sub_82469FA8` walks the chain within 120 frames but only ever keeps the recorded id: it assigns each matched node's previous level, which is the id it already has); 1 = the default (`sub_824733A0` creates the ambient behaviour with it): when the gate passes, a weighted pick from the character's profile table, else the recorded trick; 2 = a scripted list (not ported yet); 3 and up = no trick.
- Gate `sub_82469C60`: walk forward from the slot summing the nodes' frames until 300; a start node first passes only when it is an ollie / flip whose previous level is the recorded trick; a landing (airborne seen, then a node without it) passes only when the next node is not a start node and more than 50 frames passed (the landing node's frames included); the end of the window or the line passes.
- Pick `sub_82469A28` -> `sub_8245FE58`: the nollie table (profile +8) when the recorded trick's name starts with `n` / `N` (`sub_82469048`), else the regular table (profile +0); `u = rand32 / 2^32`; walk the weights (normalised to sum 1 at load, `sub_824720C8`) and take the first entry with `u < sum`, else entry 0.

Not retail yet / determinism choice: retail draws `rand32` from the skater's random source (skater vfunc +36) in tick order. Here the draw is `derive(seed, [line, node, frame])`: the same distribution, but a pure function of the NPC's seed and the slot, so the choice does not depend on how many other draws came first and a client could check it. `sub_82469A28` also keeps the recorded trick when its trick attribute record has no family entries; the per-category lists are built from every record (`sub_82DA1BF0`) and all 105 ollie / flip ids have one, so the check always passes [code + data, research b187].

## Code reference: living_world/ai_controller.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/ai_controller.rs`; each comment there links its section here.

### Avoider modes 6 and 7

The NPC skater controller's sub-modes for the obstacle avoider's modes 6 and 7 (retail TU3, evidence only; re-implemented; research b66, verified `sub_8246F818` cases 3 / 5):
- mode 6 (a low prop within 1 s): `sub_8246D560` sets controller sub-mode 4 (`ctrl+568`) when below 4; while 4 the recorded node action is dropped, the speed shape is skipped and no grab / trick is posted;
- mode 7 (blocked by a prop within 1.5 s, cap under 0.1): `sub_8246F938` (not airborne) sets sub-mode 3 and a one-shot reposition request (`ctrl+941`) on the first tick; a second tick still in mode 7 sets sub-mode 5, remembers the prop's path node (`ctrl+756`) and posts `WipeOutRequest` 1.0 once (key 0x830BECC4); later ticks post nothing;
- `sub_8246FE38` at the end of every controller tick: a pending request (offline only) re-places the skater on its line, at most once per 60 clock units: in sub-mode 3 at cursor + 5 (or the prop's node when further in mode 7), else at the cursor; then + 2; a remembered prop node + 2 within 1..19 nodes ahead wins; clamped to the line. A successful move resets the controller (sub-mode 0). The request is cleared either way.

Multiplayer: [`ControllerState`] is plain per-NPC data; the host runs it.

### Safe node picker

The safe node picker (`sub_82455728`): a node is good when more than `safe_jump_frames` (45) recorded 60 Hz frames have passed since a jump marker and more than `safe_flag_frames` (15) since a flag-0x4 node, its step slope y^2 / (x^2 + z^2) is under `safe_slope` (1.0, `0x8231A844`; 0 when the horizontal length^2 is at most `0x82063A48` 0.001) and its trick class is 0, 4 or 8; the first of `safe_run` (3) good nodes in a row wins.

### Off-board toggle

The off-board toggle the line controller presses (the push-state object `M` at brain `+92` it shares with controller B; b117, verified `sub_8246FA30` -> `824712E0`): A's activate clears it (`sub_82468AC8`: mode 0, count 0); each path tick presses first, then sets the mode for the next tick: 2 = get back on (1 = stay off, when `pc+336`, which retail never sets for ambient skaters).

### Bail respawn node

`sub_8246ED68`: the node a bailed AI skater is placed at again, picked when the bail starts (`sub_8246EE30`, `pc+880`; verified 0x8246ED68..0x8246EE24): the projection's search node (`pc+816`) plus `reposition_past`, a remembered prop's node instead when it lies ahead within `remembered_window`, clamped to the last node, then the first safe node from there ([`safe_node`]). Not ported: game modes 19 / 20 (`sub_824681D8`) use `sub_824553D8(line, cursor A)` instead.

### Track stuck

`sub_82468240` for one tick (b111 section 4): `pos` = the skater, `a` / `a_pos` = cursor A and its point, `progress` = the projection's flag (`PathFollow::progress`), `can_track` = `sub_82468070` (not scripted, not toggling on / off the board; on board not airborne, off board not mounting / unmounting). The stage climbs on no-progress ticks only; progress ticks count toward the clear.

## Code reference: living_world/controller_b.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/controller_b.rs`; each comment there links its section here.

### Controller B

Controller B (retail "NavMeshController"), the NPC skater's walk back to its line for the obstacle avoider's mode 7 ("back to the line past the prop"; retail TU3, evidence only; re-implemented; research b70 and `b78-mode7-controller-b.md`, `b79-controller-b-outputs-path.md`; verified the constants, the throttle `82467B00`, the reset `82466F40` and the writer slots `82470D08` / `82470D78`):
- the brain swaps from the line controller (A) to B (`sub_824661A8`); B's activate (`82467508`) resets it (`82466F40`: everything 0, waypoint seen -1, the "can move" latch = the skater's state now) and plans a path to the chosen line node (`82467088`): with waypoints the last one must lie within 2.0 h / 1.0 v of the node, without waypoints the node within 1.5 h / 3.0 v of the skater; a failed plan keeps A;
- B's tick (`824675C8`) writes the on-foot intents, never position or velocity: abort / hand back, the `WipeOutRequest` (key `0x830BECC4`) on a stop request or when stuck, then the path update (`82467718`: latch, re-plan, arrival, waypoint advance, stuck monitor `82466A00`), the off-board toggle presses (`824712E0`, "ToggleOffboardState": `NewToggleOffBoardState` / `ToggleOffBoardState` until the skater is off the board), the steer (`82467C90`, "OffBoardMotion" slot 2 `82470D78`: `OB_Steer` and `OB_MagHeld` = s) and the throttle (`82467B00`, slot 1 `82470D08`: `OB_Mag` and `OB_MagHeld` = min(f, 1), only when f > 0); so the skater steps off and walks back on the navmesh (NavPower in retail, the peds' navmesh here);
- B hands back to A (`sub_824661A8(brain, null)`: A re-attaches by its saved line id) on arrival (all legs done, 2.4 h / 1.0 v of the node, heading within pi/8 of the node direction), when no plan is left, or on abort.

Distances are horizontal (x, z) and vertical (y). Angles are signed about +Y. Multiplayer: [`ControllerB`] is plain per-NPC data, ticked by the host at the retail tick; [`PathService`] must be deterministic.

### Controller B path service

The peds' navmesh as B's path service (retail: NavPower, the same runtime the peds use; b79). One element per polygon crossed, as retail (`829278B8`, research b726): between two funnel corners each portal adds the point where the corner-to-corner line crosses it, clamped to the portal (`8293C158`), and a leg's end is the next leg's start (`82928358`). Element 0 starts at the raw start; a raw start more than 0.13 m in plane from its snap point gets its own element (`82928530`, 0.1 x scale 1.3, `NavRules::raw_start_offset`). So the 600-tick limit per waypoint (`82466A00`) resets at each crossing, where one element per corner let a straight walk of 50 m or more trip it while walking (b725). Only a polygon crossing itself can still trip it, as on retail. The plan's 2.0 h / 1.0 v acceptance reads the last element's end (`82467088` -> `82466930` reads +0x14) and the first aim its start (`82466860` reads +8). A `PathService` may still return entry = exit. Polygons fanned round a corner may count differently from retail (`82927B30` not read), and no recomp trace confirms the element counts. The mesh is static, so a held path stays valid (retail re-checks its element references each tick, `829277C0`). The goal is the nearest point the start can reach (retail `82928618`, b277). The start is the peds' point query, else the nearest walkable polygon at any distance (retail `82933848` -> `829338B0`: range `0x8206D108` FLT_MAX, best distance from `0x822F9310`; b324): a skater stalled on a grind block or inside an obstacle's navmesh cut can be 0.65 to 2.6 m off the mesh, which the peds' 0.5 m snap refused, so every walk-back there failed with "no plan".

## Code reference: living_world/avoid.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/avoid.rs`; each comment there links its section here.

### Obstacle avoider

The NPC skater's obstacle avoider (retail `ObstacleAvoider`, AI controller `+80`), doc 26 "NPC skater obstacle avoider". Pure: the game gathers the obstacles near one NPC skater each tick, this module turns them into retail's speed floor / cap and mode.

Retail (TU3, evidence only; re-implemented), all called from the PathController advance `sub_8246D560` once per tick:
- reset: own speed, floor 0, cap FLT_MAX, mode 0; gathering only when the global avoidance switch is clear: skaters 64 m (`sub_82464968`, type 3), peds 8 m (`sub_82463C08`, type 2), vehicles 20 m (`sub_82464000`, type 4), props 16 m (`sub_82464448`, type 5);
- entry add `sub_82463A40`: kept inside 40 deg of the facing, or inside 88 deg when closer than 1.8 m, at most 16;
- entry fill `sub_824636B0`: closing rate and time to contact (`sub_82462D00`), the lateral interval the obstacle blocks on the path (`sub_824629A8` circle, `sub_82462448` box for vehicles and props), the speed limits for moving obstacles (`sub_82463200`), the close blocker test, the skitch candidate (`sub_82462ED0`);
- floor / cap aggregation `sub_82464FA8`, skitch target `sub_824650A0`, steer target and free path gaps `sub_82465198` / `sub_82461DC8`, mode `sub_82465578`. The speed consumer is the record builder's speed shape (`sub_82470830`): mode 1 floors, mode 2 caps the AI record speed. Modes 3 and 4 move the AI's target sideways (`sub_82465280` gap choice, `sub_82464E30` offset, [`steer_point`]). Not ported (open): the prop gate `sub_82464350`, the consumers of modes 6 and 7, and an offset term of the entry add that reads controller `+128`.

Multiplayer: a pure function of positions and velocities, so the host runs it and clients consume its result (the replay tier's lag, see the game side).

## Code reference: living_world/path_follow.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/path_follow.rs`; each comment there links its section here.

### AI path projection

Retail's AI path projection (research b110): a physics AI skater's place on its recorded line follows the skater, not the recording's clock. Per tick (`sub_8246D560`): 1. the nearest node in a forward window from the search start (`sub_8246D3C0` -> `sub_82455BB0`, start..=start+4, squared 3D distance; the start only grows); 2. the projection onto the two segments around it (`sub_824563D8`), kept or replaced against cursor B with a 0.5 hysteresis, and clamped forward to A + 0.25 of A->B when it falls back (`sub_8246C930`): cursor A; 3. distance to A and the recorded lane test (`sub_8246CCC8`, node widths x 1/50); 4. off path: the rejoin commit within 0.5 m or inside the lane (`sub_8246E2B8`); 5. cursor B = A + the speed-shape ratio in recorded frames (`sub_8246E3B8` -> `sub_82456628`); 6. on path is dropped beyond 1.0 m outside the lane (`sub_8246DB50`).

Positions are (node, t): t in [0, 1] along the segment node -> node + 1. Pure and deterministic (no clock). 0. "lost" (`sub_8246D3C0` -> `sub_82468DD8`, b115): on a tick the search node did not advance, a skater that was off the path, farther than 6 m from the search node and from both lane edge points, and slower than 14 m/s is lost; the caller turns it into the reposition request (`ctrl+941`) that hands it to controller B.

NOT RETAIL YET: the 4.5 m lost test after a line-end re-pick, the node-change handler (`sub_8246BEE0`), the `G+608` and skater flag `0x04000000` in the lost test; which body point retail projects (`sub_82467FC8`) is open (ours: the deck), and so is the lane edge axis (`sub_82454648`; ours: the segment's right).

### Line end trigger

Retail [code, TU3, research b694, re-read b701 2026-10-30; constants dumped]: controller A's tick (`8246D560`) runs the line end test `8246D3C0` before the projection. The nearest forward node becomes the search node at once. On the path the line end starts when last tick's B is past the second-last segment, or on it with t above 0.8 (`0x8208824C`; B is `pc+0x2C0` / `+0x2C4`, not A as b701 labelled it: research b729, checked the loads in `8246D3C0` and `8246E3B8`); off the path when the nearest node is the last. A line end chains (`8246C7F8`) and then tests lost at 4.5 m (`0x821DDA68`) in the same call, from the chained-to node or, when nothing chained, the old search node; otherwise a search that stayed on its node tests lost at 6.0 m (`0x8208F74C`).

Ours (stage 1 of the line end port, 2026-10-30): `PathFollow::search` (the test; true at the line end), `PathFollow::line_end_lost` (4.5 m) and `PathFollow::project` (the projection); `PathFollow::tick` runs all three for a skater that never chains. The simulated NPC runs the 4.5 m test after the cursor chained at the last node or when the search reports a line end; key `PathTuning::line_end_t` (0.8). Stage 2 (2026-10-30): `choose_next_line` searches from the simulated skater's body point (`BodyGate::point`, `82467FC8`; the replay tier from its last node), and the mode 1 fallback (`82458968`, b701 R10) takes the nearest invalid start when it is no farther than the nearest valid one and that one is at least `fallback_valid_d2` (36, 6 m) away, else the nearest valid one, else the nearest invalid one; a hit needs a valid line within the radius. A simulated cursor with nothing to chain to holds on its last node (no `Finished`) and searches again every tick as retail does (b118); the advance loop stops when a step makes no progress. Retail has no leave fade at a held line end (research b702), so a held simulated skater stays until a population cull.

Stage 3 (2026-10-30, research b711 / b712): the chain runs where retail's does, in the simulated skater's line end on the trigger tick. `npc_sim::simulate` walks the NPCs one at a time in id order (only the physics steps after that loop run in parallel), so on the tick `PathFollow::search` reports the end it calls `LineCursor::line_end_chain` (`choose_next_line` from the body point with the mode 1 fallback, the switch from wherever the cursor is, mid-segment included), records the `BranchRecord` and the `Branch` event, resets onto the new line (`switch_to`, the line state, the avoider controller), moves its own entry in the others / in-use list (so a later NPC at the same line end that tick takes another line, b620), then runs the 4.5 m lost test on the line it is on, then the projection. The valid-line cache moved from `advance`'s local scratch into the `NpcValidLines` resource (filled by `advance`, read by `simulate` the same tick). Removed: the cursor-side retry at the last node (`retry_chain`) and the guess that a branch record meant a chain. Clients: a mirrored cursor applies a host record keyed on its frame, line and node at the top of `step`, so a mid-segment switch reaches it (`LineCursor::apply_switch` is shared by branches, chains and mirrored records).

The score heading (stage 3 item 2, research b706; checked `8246C3C0` and `824536C8` in Ghidra 2026-10-30): the branch and line-end angle check takes the effective deck forward (`82592A00`), also when biped or wiped, and the signed angle about the skater frame's up row (`824536C8` projects both vectors onto the plane across the axis). Ours: `BodyGate::heading` = (`effective_deck_forward`, the animation root's up row), used by `branch_score` (`angle_about`); without it (the replay tier) the context's forward about world Y, bit-identical on flat ground.

Walk-back on the same tick (b701 2g): `take_reposition` / `ControllerB::activate` run after the follow block, so the lost test's request starts the walk-back that tick (it ran before the follow block, one tick late); the walker first ticks next tick (retail controller B timing not measured).

Mod keys: `skater_path {line_end_t, lost_radius, line_end_lost_radius, lost_max_speed}` (`PathTuning` on `npc_simulated.path`, retail 0.8 / 6 / 4.5 / 14) and `skater_line_chain.fallback_valid_m` (6 m, stored squared), read back and reset on mod disable (`modding::world_tuning` test `npc_line_end_settings_are_mod_reachable_and_reset_on_disable`).

The up axis (research b713, checked `82592AF0` and `82BE3650` 2026-10-30): `8246C3C0`'s `vtbl+0x1C` call (`8258E888` -> `82592AF0`) copies the physics output record's `+0x1B0` block, which `82BE1AE8` fills from `82BE3650`, the effective animation root (the skeleton root at `character+0x2E90`, rows 0 and 2 negated when `+0x9AC & 4`); its row `+0x10` is the root's up. Ours: `effective_root` (the same `flags_2476 & 4` flip), row 1. Open: retail reads the copy published at the end of the physics update, ours the live root (possibly one tick apart).

Which lines a line end may take (research b707 / b715; checked `82458860`, `82458038`, `82457620`, `82458968`, `8246C7F8` 2026-10-30, plus a recomp trace of 7255 line tests): the mode 1 search walks every line and skips one only when the AI's path filter rejects it (`*(pc+4)+0x2C`, a pattern match on the 16-byte path id with wildcards; ambient skaters' pattern passed every line) or when a line flagged 0x200 overlaps a nearby trigger volume; neither the search nor `8246C7F8` excludes the skater's own line or a line another skater rides. Ours: `choose_next_line` dropped its own-line and in-use skips (the crowd term of the score still counts skaters near a candidate node), so a simulated skater alone in the world falls back to its own line's start, as retail. The junction branch (`choose_branch`, `sub_8246BEE0`) tests its targets with the same `82458860` (the trace's caller lr 8246C0AC lies inside it), so its in-use skip was dropped too (2026-11-08: racers following one another on race_dwtn_02 were pushed past the course's junction into a dead end). Not ported, no outcome to port: the "world object" is a challenge trigger volume within 40 m of the search point (research b719, agent read, radius not verified), and none of the 1691 exported lines of the three cities carries flag 0x200 (counted in `skater_paths/index.json`). DLC lines are not exported, and runtime writers of the flag were not all read.

Clients following a simulated NPC (b711 O1 and item 2): the state a client needs is the host's `NpcReplay::follow_target` (its cursor clock, replicated each tick), `branches` and `tricks`. A host restart of the cursor (re-place, re-snap, bail respawn, walk-back hand back: `npc_sim::restart_cursor`) is `LineCursor::respawn`: the spawn state with the frame clock kept running (frames never repeat, so records stay unique), recorded as a `BranchRecord` with `respawn` set. A client applies records at its current frame in `LineCursor::apply_records`, run from `step` and once a tick before stepping (a restart on a tick the host's clock stood still). `LineCursor::hold_at_end` is set by host and client from `follow_target`, so a mirrored cursor holds at a dead end and takes the host's chain record when it comes. Test: `replay_tests::a_client_follows_a_simulated_cursor_through_a_chain_a_restart_and_a_hold` (host and client cursor equal every tick through a mid-segment chain, a restart and a hold). The wire itself is not built (no networking until asked).

Tests: `path_follow::tests::the_line_end_starts_past_point_eight_of_the_second_last_segment`, `a_line_end_chain_tests_lost_at_four_and_a_half_metres`, `replay_tests::the_line_end_search_starts_at_the_body_and_falls_back_like_mode_1`, `a_simulated_cursor_holds_at_a_dead_end_and_the_replay_tier_finishes`, and `replay_tests::line_end_stage_3::*` (the chain on the trigger tick from mid-segment, a far fallback chain raising the lost test, no next line holding, two skaters taking different lines in id order, a mirrored cursor applying a mid-segment record once), `replay_tests::the_branch_angle_is_taken_about_the_skaters_up_axis`.

## Code reference: living_world/ai_push.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/ai_push.rs`; each comment there links its section here.

### AI skater push

The AI skater's push: retail's PathController posts the same push intents the player's pad listener does while the skater is slower than its line wants, so an AI skater plays the normal Push animation (doc 26e "AI skaters push when slower than their line", research b341).

Retail (TU3, evidence only; re-implemented):
- `sub_8246BAA8` (every board tick, from `sub_8246FA30` on the ground, not in the air): the target speed is the recorded step x 60 (`pc+0x1F0`), floored at tunable `8213474C` (4.0 m/s) when the skater is more than tunable `A8027415` (2.0 m) off the line, then shaped by the avoider (`sub_82470830`, ours [`super::avoid::AvoidState::shape_speed`]). At or above it the brake writer runs (`sub_8246B810`, not ported [`brake`]); below it the push writer `sub_8246B4C8`.
- `sub_8246B4C8`: no push in trick categories 4, 7, 10, 12 (`pc+0x340`), stuck stage 4 (`pc+0x238`), or when the line direction (`pc+0x26C`) points against the board's forward. The factor is 0.97 (0.99 when `sub_824681D8() == 23`, mode not decoded), or 0.3 when a start-trick node is under 40 recorded frames ahead (`pc+0x31C`, `pc+0x308`) and the speed is above 2.0 m/s; x 0.3 when an off-board node is under 60 frames ahead (`pc+0x39B`, `pc+0x354`), x 0.3 on a slope under -0.22 (`sub_8246B9C0`), x 0.3 above 11.0 m/s. No push at or above factor x target, or with a heading error of tunable `86923C10` (40 degrees) or more. Else `RightPush` or `LeftPush`, `NewPush` and `Pushing`, all 1.0, every tick the condition holds (no timer).
- The foot (research b347, verified both getters): `RightPush` when the SkaterAnim component's (type `0x82453410`, "ISkaterAnim") stance-mirror bit (slot +0x1C `82B97140`, flags bit 30) differs from its riding-fakie bit (slot +0x0C `82B970D8`, bit 29), else `LeftPush`: regular forward pushes right, goofy forward left, fakie flips it. Ours: the same two bits of `SkaterAnimation::stance()`.
- The brake writer `sub_8246B810` (b347), at or above the target, see [`brake`]: `Brake` (or `ManualBrake` in a manual, category 8) with a ramped magnitude, only far above the target speed.

## Code reference: living_world/npc_avoid.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/npc_avoid.rs`; each comment there links its section here.

### NPC avoid host

NPC skaters avoid peds, cars, props and other skaters with retail's obstacle avoider (`skate_core::living_world::avoid`, doc 26 "NPC skater obstacle avoider").

Each world tick, before the cursors advance, the host gathers the obstacles (peds and their velocity from the last tick, traffic cars with their box, props with their box, the other NPC skaters and the players) and runs the avoider for every riding NPC skater. Its answer goes where retail's goes:
- the speed (retail: the AI record speed, `sub_82470830`): the replay tier holds the cursor back by the missing recorded frames (`NpcAvoid::lag`), so a cap of 0 stops the skater and a floor only catches up held-back frames (a recording is never played faster than recorded);
- the steering (retail: the target moved across the path, `sub_82464E30`, and the board pulled towards it at 2 cm a tick, `82C05EC0`): a lateral offset of the drawn skater that moves 2 cm a tick towards the chosen gap and back to the line once the path is clear. The simulated tier reads the same output for its AI record (`npc_sim`).

Air, off-board and trick spans are never held back or moved (the recorded jump would break). Multiplayer: only the host evaluates; `lag` and `offset` are plain per-NPC state a client would take from the host (no replication yet). Logs `NPC_AVOID` on every mode change.

## Code reference: riding/grounded/state/board_path.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/grounded/state/board_path.rs`; each comment there links its section here.

### Board path 82C05EC0

`PhysState_PhysicsGround` / `SlideGround` board path `82C05EC0` (doc 26, "NPC skater steering"): the external (AI) physics record at Processed `1616` pulls the deck toward the recorded pose every tick.

Retail (TU3, evidence only; re-implemented): `UpdatePostPhysics 82D387A8` runs the ground wipeout check `82D8F9E0`, then, while record flag bit 31 ("steer to target", Processed `1776`) is set, `82C05EC0(controller, physics_ai)`:
- bit 30: position `82C056B0`: `d = (target - deck) * gain`, the deck translation moves by `min(|d|, max_step)` along `d` (only when `|d| > 1e-6`), set on part 6 alone (`82D9C8C8` -> `82BD4318`);
- bit 28: velocity `82C05868`: the deck body's linear velocity moves toward the target the same way (`gain`, `max_change`);
- bit 29: facing `82C05988`: the target forward in the ground frame (`controller+292` -> `+752`) with y = 0 and normalised, its signed angle from +Z about +Y (`8296EC98`) wrapped to [-pi, pi], times `gain`, clamped to `max_degrees` (deg -> rad by `0x8206D110`); the deck rotation becomes `D * F^T * Ry(step) * F` about its own position and the whole board is set (`82C0B2C8`).

The gains are the `physics_ai` vault record (`*(*(0x830CFDA4)+272)+4`, class `527C93F55CFC663D`; `default` record values below). The player never has the record: its Processed `1776` stays clear, so this never runs for it.

## Code reference: living_world/npc_reactions.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/npc_reactions.rs`; each comment there links its section here.

### NPC wipeout reactions

NPC skater reactions to other skaters' wipeouts (the reaction manager, research b166 / b168) [code]:
- each simulation tick (`sub_8245A7E8`, tick mask bit 0) every skater's reaction bits are cleared first (R+0x24 `clrlwi 7`, verified), then set from this tick's events, so a pulse lasts one tick;
- a skater's wipeout start (`sub_82772748`: H+0xD1 = !prev && cur of the wipeout byte, Processed `+2468` bit 18) is passed to every other skater within a strict 15.0 (`0x822F8F68` = 225.0 compared with the squared distance, `sub_8245E950`, verified) as the slam bit with "by" = the sender's model (`sub_8245E6C8`, `sub_8245E740`; 0x61 = 97 when the sender has none); its wipeout end (H+0xD2) is passed the same way as slam_b (`sub_8245E848`);
- the skaters are visited in index order, so the last sender in range names "by";
- a skater's own crash bit is its wipeout byte itself, a level (`sub_827A1B78`, entry +148 bit 5). Trick seen (2026-10-29): a skater whose score `[P+0x3C]+0x88` is above 50 (`Hash_4BEEDCA4AD799796`, strict signed) raises trick for every listener in range, a level each tick while it stays above. The score's only writer is the scoring publish `82DA6630` [code, recomp asm store search + Ghidra]: the larger of the last trick sequence's reward (field 9, written at each sequence end by `82DA6538`) and the sequence in progress (field 8), truncated, minus field 13 (`+0x34`, reset to 0 by `82DA5C78`; its other writers unread, taken as 0). Ours: `ScoringRuntime::reaction_score` (`ScoreHolder` `last_reward` / `accumulated`) per observer into `npc_reactions` (`trick_threshold` is data); NPC skaters score 0. Test `a_score_above_50_is_trick_seen_within_15_m_every_tick`; not heard in game yet.

Ours: the skater order is the host's stable key order (players first, then NPC skaters by id), and players listen to nothing (only NPC skaters speak these lines). Multiplayer: a pure function of the skater list and the previous tick's wipeout states; the host runs it and sends the raised reactions.

## Code reference: living_world/ground_steer.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/ground_steer.rs`; each comment there links its section here.

### AI ground steer

Retail's analog steer of an AI skater on the ground (research b121): `sub_8246AA00` turns the skater's heading against its line into the error `pc+824`, and `sub_82471070` writes it as stick input to `Turn`, `BodySpin` and `KickTurn`. It runs on and off the path: off the path (beyond 0.4 m) it aims from the far look-ahead point toward the near one, all near from 3 m, which turns a skater back to its line while the steering record carries no target (`sub_8246DB50`).

Pure and deterministic. Retail constants are the tuning defaults (data, a mod may change them).

## Code reference: living_world/air_steer.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/air_steer.rs`; each comment there links its section here.

### AI air steer

Retail's analog steer of an AI skater in the air (research b123): `sub_8246ACF0`'s airborne arm `sub_8246B358` turns the character toward the recorded skater forward at the near look-ahead (`pc+208`, turned 180 degrees when the takeoff latch `pc+927` is set), and `sub_82471188` writes the error as stick input to `Turn`, `BodySpin` and `KickTurn` outside a dead zone. The latch (`sub_8246A700`) is set on the takeoff edge when the character faces against the current node's recorded skater forward.

Pure and deterministic. Retail tunables are the defaults (data, a mod may change them).

On by default since 2026-10-25 (retail has no gate on the call; b616, ask 354): with it off, sim skaters landed recorded 180s and quarter pipe airs backwards and were bailed by the stuck tracker. Mod keys and the A/B are in [26c "Air steer"](26c-living-world-npc-skaters.md#verification).

## Code reference: physics/board_path.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/board_path.rs`; each comment there links its section here.

### Board path game side

Board path `82C05EC0` in the ground states' post-physics (`82D387A8`): the external (AI) physics record pulls the deck toward its target (`skate_core::riding::grounded::state:: board_path`). Runs only while the record asks to steer (Processed `1776` bit 31); the player's record is never set, so the player's physics is unchanged. NPC skaters with a physics body (living world, simulated tier) set it.

## Stall lines carry the position (2026-10-22, research b523)

`NPC_SKATER_STUCK ... (stage 2)` now ends with `at=[x, y, z] dist=` and `NPC_SKATER_LOST` with `at=[x, y, z]` (the
skater's projected point): b523 counted 15 LOST and 9 stage 2 stalls in the three newest laptop sessions but could not
place them without positions. Its findings (drift off the line at speed is the common start; two re-snaps jumped 31
nodes back) are in research b523; the retail checks it proposes are open.
## Lateral ratio and the steep re-snap scan as retail (2026-10-22, research b524)

- **Lateral ratio (`pc+868`, `sub_8246CCC8`, asm-checked):** retail divides the distance to A by the NARROWEST
  same-side lane width of A and the next 4 nodes; the lane flag (`pc+932`) still compares with A's own width. Ours
  divided by A's width only, so where the lane narrows within 4 nodes our skater still counted as "centred" (ground
  steer aiming at the far look-ahead) while retail already blends toward the near point. Change: `path_follow.rs`
  (`PathTuning::lane_lookahead`, default 4). Test `the_lateral_ratio_uses_the_narrowest_width_ahead`.
- **Steep re-snap scan (`sub_8246D168`, asm-checked):** retail tests "node lower than the search node" before the
  240-frame budget, and both only past `search + 2`; ours tested the budget first for every node, so a long first
  segment ended the scan before a lower node. Change: `ai_controller.rs` `resnap_node`. Test
  `the_steep_resnap_finds_a_lower_node_after_a_long_segment`.
- **Matches retail already (b524):** the stage 2 re-snap's nearest node is a whole-line, node-point search, first on a
  tie (`82455108`); the 73 -> 42 jumps in the 2026-10-21 laptop sessions are retail's choice too. The lane flag has no
  hysteresis in retail either.
- **Lane side (`pc+932` / `pc+868`, `sub_8246CCC8`, asm-checked, research b526):** retail takes
  s = (P - A) . (d x U), U = (0, 1, 0) (0x82139A20); s < 0 reads width byte +0x25, else +0x26. With the line heading
  +z, a skater at +x reads +0x25. Ours used the mirrored vector, so a skater on one side was measured against the other
  side's width (at University nodes 68 to 75 the widths are 5.1 m vs 1.2 to 4.6 m). Fixed in `path_follow.rs`; test
  `each_side_of_the_line_uses_its_own_width`. Retail's lost test (`sub_82468DD8`, b527) puts the +0x25 edge on the
  OTHER side (A + n * width, n = d x U): retail is inconsistent between the two, and ours keeps each as retail (the
  lost test was already right).
- Not play-tested; whether the lateral fix reduces the University drift (b523) needs a session.
## The avoider's side and the lost test's direction as retail (2026-10-22, research b527)

- **Avoider side (`sub_82454798`, asm-checked; `sub_82464E30` / `sub_82462448` per b527):** retail's lateral axis
  is d x up; a negative lateral uses the left width (+0x25) and moves to that side (heading +z: +x). Ours used the
  mirrored axis in both copies (`avoid.rs` `right_of` and `npc_avoid.rs` `right_of`, which maps the avoider's offset to
  the world for the steered sample, the rendered skater and its root), so steering put each width on the other side.
  Both flipped together. Tests: `steer_point_puts_each_width_on_its_own_side`, the gap test now expects the left gap
  at +x. Not ported: b527's finer box-corner rules in `sub_82462448` and the zero-width case of the steer point.
- **Lost test direction (`sub_82468DD8` / `sub_82454648`):** the lane edge points come from node n's own step,
  normalised in 3D (ours took the next node's step, normalised flat). The side was already retail's. Test
  `the_lost_test_uses_the_nodes_own_direction`.
- Not play-tested.
## The record's lane flags as retail: NPC skaters steer back onto their line (2026-10-25, USER-ASKS 345, research b600 to b602)

Problem (user: NPC skaters "run around trying to get onto a line far more than retail ever does"; laptop sessions of
2026-10-25, 5 sessions, about 53 min): 42 `NPC_SKATER_LOST` walk-backs against 10 stuck stage 3 ones. The lost /
walk-back rules, the line-end chain, the take-off arc and the air already match retail (b600, b601: lost radius 6.0 /
max speed 14, `path_follow.rs`; the arc window 4.0 / 0.333 / 3.0). The skaters reached jumps 2 to 5 m to the side of
their line (still inside the lane, so "on path") and at 40 to 75 % of the recorded speed, so the recorded arc was
rejected and they landed off the line.

Cause [code, both sides]: retail's per-tick update `sub_8246FA30` calls the ground steer `sub_8246AA00` and then the
record builder `sub_8246DB50` in the same tick (recomp `skate3_recomp.8.cpp` 56672 / 56973). `sub_8246AA00` stores
`pc+944` = inside the lane (`pc+932`) and lateral (`pc+868`) under 0.6 (centred) and `pc+945` = not avoid-steering
(`pc+933`), trick category not 9, not centred and distance (`pc+828`) over 0.4 m (rejoining) (`8.cpp` 44225-44275).
`sub_8246DE38` reads both for the record bits (above: B needs `pc+945` clear, C needs `pc+944` clear): while rejoining
the board path's velocity and facing pulls are off and the ground steer turns the skater back. Ours never set either
byte (`npc_sim.rs` set only `byte_933`), so all pulls ran on every on-path tick and held the skater parallel to the
line, 2 to 5 m out.

Change: `ground_steer::lane_flags` (the two flags; `heading_error` uses it too, so they cannot diverge);
`npc_sim.rs` works them out from the path follow before the record on ground ticks and keeps them through the air
(`NpcSim::lane_flags`), and sets `byte_944` / `byte_945`. The 0.6 lane ratio and the 0.4 m rejoin distance are the
existing `GroundSteerTuning` data (`lane_centre`, `rejoin_start`). Category 9 is still not mapped (as the ground steer,
b121 open).

Test: `ground_steer::tests::lane_flags_mark_centred_and_rejoining_as_retail` (centred, 1.5 m off, 0.3 m off,
avoiding, category 9); the record bits from the flags: the `ai_record` tests.
Open (b602): the speed loss is partly inferred (the skid between the full-stick steer and the pulls); the avoid point
written back into the steer's far aim (`sub_82464E30`, `pc+352`) and the low prop rule are smaller parity items; the
landing loss of skater #7 at node 26. Not play-tested.

## The far aim moves into the gap while avoid-steering (2026-10-28, research b612)
- Problem: while an NPC skater avoid-steered (mode Steer), the ground steer still aimed at the far line point 45 ticks
  ahead, so the stick pointed down the line instead of into the chosen gap and the skater dodged less than retail.
- Retail [code]: `8246D560` builds the far aim `pc+352` (A + 45); in avoid modes 3 / 4 / 5 it calls
  `82464E30(avoider, &far, A + 15)`, then `8246AA00` steers at the moved point. Before the steer target's path point
  the far point shifts by `lateral` x the lane width on that side (`82454798`); past it the anchor is the target's path
  point, or A + 15 once that is past the target too (`8245A100`). Widths on the result are 0. `82454798` with a zero
  width shifts `lateral` metres on the unit side.
- Change: `avoid::avoid_aim` + `before` (skate-core `living_world/avoid.rs`), the zero-width edge in
  `avoid::steer_point`, `path_follow::path_point`, `npc_sim.rs` `avoid_far` feeds the ground steer's `far`.
  `GroundSteerTuning::avoid_anchor_ticks` (retail 15.0, `0x820BD16C`).
- Moddability: `living_world.npc_simulated.avoid_anchor_ticks` (0..=600), reset on mod disable.
- Tests: `avoid_aim_follows_both_retail_branches` (skate-core), `npc_avoid_anchor_is_mod_reachable_and_reset_on_disable`
  (skate-game).
- Open: retail uses the path point's up vector, ours y-up (only differs on steep lines); whether retail runs the ground
  steer during skitch (mode 4) is unchecked, ours skips it. Not play-checked.


## Push while riding fakie

**Problem.** Laptop session game-20261101-101251: 66 NPC bails in 26 minutes, 25 of them the stuck chain's stage 5 give-up, mostly off the path at about 1 m/s. 24 of those 25 had `riding_fakie` backwards lines in the run-up. A skater that landed fakie could not push.

**Root cause.** `sub_8246B4C8` takes its direction gate from `sub_82468FC0`: the effective forward, negated while `[PhysOut+56]+155` (riding fakie) is set (read in Ghidra, b200). Its heading gate reads `pc+824`, the ground steer's error, measured from that same facing (b353). Ours used the effective forward without the negation for both, so a fakie skater read about 180 degrees and never pushed.

**Change.** `ai_push::against_line` applies the negation. The push and brake heading gate use this tick's ground steer error when the steer ran, else the angle from the fakie-corrected facing. Values stay in `npc_push`; no new constant. Multiplayer: same inputs, deterministic.

**Anticipation.** The trick signals' heading is `pc+824` too: `sub_824696B8` posts `AnticMag` only under `|pc+0x338| < pi/4` (read in Ghidra). Our steer runs after the signals, so they read last tick's value. `sub_824691A0` gates the anticipated trick (`pc+796`) on the off-line distance alone, so `ai_signals::anticipated` lost its heading and open-trick gates.

**Verification.** `a_fakie_skater_travelling_along_the_line_is_not_against_it`, `the_anticipated_trick_ignores_heading_and_an_open_trick`. Muted 300 s University run, before and after the push change: bails 15 -> 10, give-ups 4 -> 1. Muted 240 s runs with the anticipation changes switched separately, University community skate park and DownTown (`tools/dev-bench/npc_matrix.sh`), totals over both: neither 19 bails, 1 give-up, 293 landings; heading only 19, 4, 270; gates only 19, 4, 270; both 17, 6, 276. No combination wins at both spots: University favours neither, DownTown favours both. NPC runs diverge after the first few bails, so a difference this size between builds is noise. The code reads decide these changes, not the runs, and neither set shows the push change's effect at a measured rate. A play session is still the check.
**Writer gates (`sub_824696B8`, runs inside the dispatcher `sub_8246A2E0`, so after the push and with the slot as dispatched).** `AnticMag` / `AnticAngle`: anticipated ollie or flip, `pc+0x39E` clear, `|pc+0x338|` under pi/4, slot category not 8; an open trick of another category does not stop it. `Crouch`: node flag 0x02, `pc+0x39E` clear, no anticipation posted, slot category 0. `Manual` ahead: upcoming category 8, slot category not 8, and `pc+0x3A0` (the airborne byte, copied from `+0x1B6` in `sub_8246A700`) or `pc+900` under 5 (ticks since the landing edge, same function, starts at 7200). `pc+0x39E` is set by the push writer `sub_8246B4C8` and cleared each tick in `sub_8246F6E8`. Ours: `ai_signals::signals`, `drop_for_push`, `NpcSim::since_landing`. Test: `the_writer_gates_anticipation_on_a_manual_and_on_the_push`. Checked against the code only; no session has tested it yet.

**Open.** Node 60's reason 3 bails are unchanged: 63.3 degrees against the 63.0 limit (`WIPEOUT_REASONS deck_error`), cause open. `pc+0x26C` as the line direction is inferred (b341).

## Wipeout reason 31 and 2 diagnostics (2026-11-02, research b802 / b803)

**Problem.** NPC skaters bail with reason 31 off the board (BipedGround) and with reason 2 in KnownAir (5 at University nodes 83 to 85, one spot, about 1 m off the recorded line); the logs did not say which check fired.

**Retail.** Reason 31 on foot comes only from the BipedGround post `82D32AA0`, and only while the animation flag `IsStumbling` (0x400) is set: turn start plus wants air, or wants air, start angle over pi/2, steep ground kind 2 / 4, or up y under 0.5 ([code], b802). The air post's four reason 31 sites run only in BipedAir. Reason 2 comes from the closing velocity check `82D90AB8` (called by the ground, foot plant, air and grind posts): xz length or |y| of the closing velocity in animation space over the limits that `82D90358` picks (trick: side-on and jump fix frames over 4; wheels; air), xz halved on a body flip and tripled on a light DMO ([code], b803). Ours matches both in conditions, constants and order (`ground_lifecycle/post.rs`, `wipeout/air.rs`, `wipeout/common.rs`); `offboard/air_collision.rs` is not compiled.

**Change.** Log lines only: `BIPED_GROUND_R31` (which stumbling condition, `physics/biped_ground/lifecycle.rs`) and `AIR_R2` (branch, closing terms against the limits, jump fix frames, flags; `physics/wipeout.rs`). `closing_limits` / `closing_terms` are shared helpers so the log uses the same math as the check.

**Open.** Which condition fires for NPCs (needs a session); why the University NPCs are 1 m off the line and flicker KnownAir -> PhysicsAir -> KnownAir at node 83. Evidence: research b802, research b803.

**Update 2026-11-03 (session game-20261103-152610, DownTown).** The first `BIPED_GROUND_R31` from a session: `turn_and_air=false wants_air=false start_angle=2.772 ground_kind=0 up_y=1.000`, so the start angle over pi/2 fired. The chain: at node 41 the AI profile swapped the recorded ollie for a fs 360 pop shove-it (legal: the gate `82469C60` passes with 55 recorded frames over 50, ours matches, research b812), the board met a ledge about 0.9 m above the line side-on (`AIR_R2 branch=trick`, the 0.1 m/s trick limit), the skater came down on its feet nearly backwards and bailed with reason 31. The recording clears that ledge; ours flew 0.15 to 0.2 m lower (research b813).

**Recorded take-off log (2026-11-03).** `AIR_RECORDED_TAKEOFF` (`physics/air_trajectory/mod.rs`, log only): at an AI take-off that carries a recorded arc, whether retail's accept test (`82D67B50`) took it, our launch velocity and speed against the recorded ones, the squared distance from our first step to the recorded start, the origin and the recorded start. A muted 300 s DownTown run: 252 take-offs, 145 accepted, 106 more than 2 m from the recorded start (hops elsewhere carrying a stale arc, which fly our own arc as in retail), 1 rejected for its speed ratio. Of the accepted ones our launch speed is 0.93 of the recorded at the median and below 0.8 for 29 of 145; retail keeps the body's own launch speed when it casts the recorded arc, so a slow take-off flies short and low. **Open:** why our NPCs reach recorded jumps slower than the recording (the node 41 case lost 8.1 -> 5.8 m/s in the trick windup); retail's take-off error (selector +2816) and record offset (+2800, record vector 9), which ours does not keep and whose readers are not found yet.

**Pop speed logs (2026-11-03, research b814).** GroundAnimation's jump (`82D93618`) builds the launch from the prepared jump velocity (Processed +704), which follows the body only while the animation sets PrepareJump (2468 bit 21) and otherwise keeps its value with slope gravity (`82DB4048`); when it differs from the body by more than 7 (squared) the launch is the body velocity clamped to |prepared| x `JumpSpeedScalarVsAngle`, so the clamp can only slow a launch. The AI speed pull (`82C05EC0`) runs only in PhysicsGround / SlideGround, in retail and ours. Two log-only lines: `NPC_GROUND_JUMP` (`physics/ground_animation/board.rs`: launch, body and prepared speed, their angle, the squared difference, PrepareJump on the launch tick) and `NPC_TAKEOFF_TRACE` (`living_world/npc_sim.rs`: 40 ticks after each trick dispatch, state, measured speed, node, PrepareJump, record flags, action and motion graph states). Muted 300 s DownTown runs: of 99 launches none came out below 0.8 of the body speed (66 took the clamp branch, PrepareJump is off on the launch tick itself); of 122 tricks with a take-off, PrepareJump was set before the pop in 117, and 12 lost more than 20 % between the dispatch and the take-off, almost all over the 5 or 6 GroundAnimation ticks of the windup (for example 8.7 -> 5.5 m/s), one at GroundAnimation entry (7.4 -> 0.8 m/s in two ticks). **Open:** which GroundAnimation force (`82D33E30` foot / brake / friction / collision) slows those windups.

**Where the approach speed goes (2026-11-03, research b815 to b818).**
- Only 3 of 17 slow take-offs lose their speed inside GroundAnimation. Those meet a ledge mid-windup and the collision response stops them (tag 15, gated by `82D944E8` on 2472 bit 0x20000, ported 1:1). The two jerry_hsu cases (line `...e503`, node 7) are not too fast: the line is recorded at 8.1 to 8.4 m/s up to the trick node and drops to 3.7 m/s only at the pop, a small hop onto a ledge (start velocity 3.1 m/s forward, 3.2 m/s up); b815 compared our approach with the recording's speed after the pop. Both pops started 0.17 to 0.21 m from the recorded start, cast the recorded arc and landed aligned at node 13 (`AIR_RECORDED_TAKEOFF`, `NPC_SKATER_LAND`).
- The others launch at body speed; on an accepted recorded arc KnownAir then follows the arc's speed, which is retail (`82D352D0` writes no velocity, ours matches); our speed trace measures the deck rigid body in every on-board state, so this is real motion, not a measuring artefact (b816).
- At a line-end chain retail writes no velocity (`8246C7F8`, `82468AC8`); the only speed lever is the board path pull `82C05868` (min(0.5 |dv|, 0.2) per tick), and only while the record's velocity bit is set, which both clear while rejoining. After those chains the skater is flagged rejoining for about half a second and is back on the line (about 0 m off) within a second (`NPC_SKATER_REJOIN`), so the pull runs for most of the run-up.
- `82467FC8`, the AI's body point, is the deck rigid body's position (board part 6), the same point ours uses for the push gate, the speed source, the rejoin distance and the lateral error (b818).
- New log `NPC_BOARD_PULL` (`physics/board_path.rs`, log only, when the deck is more than 1.5 m/s off its target): muted 300 s DownTown run, 1584 ticks. The pull changes the deck speed by a median 0.18 m/s per tick, but the deck keeps only a median 0.09 m/s per tick toward the target, the same faster or slower than the target, so half of each pull is undone before the next tick (the pull writes the deck body only).
- **Retail keeps the same half [trace].** A recomp hook on the velocity step `82C05868` (`AIPULL`, research-hooks, category `aiskater`, the same 1.5 m/s filter), a muted 6 min idle run at DownTown [-221.8, 40.0, 1.7] with the player off the board: 927 pulls by 3 NPC skaters, gain 0.5 and cap 0.2, one pull per skater every 16.6 ms (60 Hz, as ours). Applied a median 0.19 to 0.20 m/s per tick, kept 0.09 to 0.10 m/s per tick toward the target (faster than the target 0.098, slower 0.090), against ours 0.18 applied and 0.09 kept. So the pull and what survives of it match retail; the half lost to the board solve is retail behaviour, not ours.
- **Open:** the take-offs that launch below 0.8 of the recorded speed (29 of 145 accepted in the 300 s DownTown run, the node 41 bail among them: ours 5.8 m/s against the recorded 7.84). The windup, the launch clamp, the pull, the line-end chain, the body point and the KnownAir hand-over all match retail; which of those 29 cases are real losses (as at node 41) and which follow recordings that slow down themselves is not sorted yet.
- **Sorted with `NPC_TAKEOFF` (2026-11-03).** New log line (`living_world/npc_sim.rs`, log only): on the first air tick after a trick dispatch, the NPC, its line node, our deck speed, the recording's own speed at that node and the node before, and the nearest recorded jump's start speed. A muted 600 s DownTown run, 241 take-offs: 188 at the recorded approach speed, 19 below the recorded jump speed but at the approach speed (the recording slows down there itself), and 33 real losses below 0.8 of the recorded approach (ratios 0.40 to 0.80; 20 in PhysicsAir, 13 in KnownAir). That first count measured on the first air tick, which includes the KnownAir hand-over dip (b816): at node 32 all six NPCs launched at 8.3 m/s, above the recorded 7.33, and node 24 was a pop out of a 5-0 grind. The line now also gives `launch`, the GroundAnimation launch speed recorded during the windup, and `from`, the state the take-off came from. A muted 300 s DownTown run: of 102 take-offs from GroundAnimation, 20 launch below 0.8 of the recorded approach (6 more are slow on the first air tick only); since the launch never falls below 0.8 of the body speed, those 20 were already riding slower than the recording before the trick. 13 of the 20 are within the first 25 nodes of a line, where a new NPC starts from the spawn push (0.75 of the recorded speed, as retail); the others (nodes 56, 60, 93, 135 to 137, 189) are open. Second machine (the laptop, 600 s muted runs on 66fc2419, the first-air-tick measure): DownTown 35 of 254 take-offs below 0.8 of the recorded approach (14 %, the desktop 33 of 241) at the same repeating spots (nodes 21, 25, 32, 137), 15 bails, 0 panics; University 0 of 160 (all at 0.88 to 0.97 of the recording), 12 bails, 0 panics. Same runs on 80424d68 (with `launch`): DownTown 36 of 211 GroundAnimation take-offs launch below 0.8 of the recorded approach (17 %, the desktop 20 %), 12 of them in the first 25 nodes, node 137 again with four NPCs; University 95 of 160, all one jump at node 5 just after the spawn, launching at 6.45 m/s = 0.75 of the recorded 8.6 (the spawn push) and riding the recorded arc at 8.3 m/s from the next tick. `launch` is blank for take-offs that did not come through GroundAnimation, by design, and for 8 GroundAnimation ones that recorded no jump. Node 137 is no loss either: the line (`...4182`) rides up a kicker from the trick node 132 and leaves it at node 136 with a recorded jump start velocity of 8.84 m/s; the step speed of the take-off node (11.67) is the jump transition, not the approach, and ours launches at 8.03 to 8.17 (0.91). Measured against the recorded jump start speed where a jump lies within 6 nodes, the laptop DownTown run has 27 of 203 GroundAnimation launches below 0.8, 11 in the first 25 nodes. The worst later one (andrew_reynolds, line `...9244` node 58 -> 60: 3.38 against 7.85) pops onto a 0.4 m ledge 2 frames after the trick node in the recording, while every traced NPC pop comes 8 PhysicsGround plus 5 to 6 GroundAnimation ticks after the dispatch (about 0.23 s), so ours reaches the ledge face mid-windup. The 8 + 5 to 6 ticks are the stock ollie windup, not a delay of ours (research b820): the trick intent reaches the graphs on the dispatch tick, `FromAntic` plays the `_G` clip (not interruptable), PhysicsGround becomes GroundAnimation when the animated board lifts past `animated_board_threshold` (`player/selector/ground.rs`, TU3 `0x82BDC6C8`), and the take-off follows on `_A`; retail's dispatch `8246A080` / `82469448` adds no timer or early fire for ollies and flips (lead 0, b188), so retail spends the same windup. The recordings agree: from every start-trick node to its first airborne node (`npc_line_dump <pack> gaps`), DownTown 1696 pops with a median of 19 frames (70 % at 14 or more, 6 % at 5 or fewer), University 1661 with a median of 17 (68 %, 8 %). Node 58's 2-frame gap is one of the few short ones; with the windup retail plays it pops about 1.9 m later too. So the remaining slow launches past the spawn ramp are recording outliers, not a port difference. Not proven with a recomp hook (the hooks b820 names: `8246A2E0`, `0x82BDC6C8` and the ground selector, near line `...9244` node 58).