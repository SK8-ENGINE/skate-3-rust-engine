# 26g: Living world: Pedestrians: props and skater contact

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Peds walking through props, 2026-10-05

**Problem.** User: "Peds ... also completely ignore the props in the world and go right through them." (Same for
NPC skaters: "npc skaters ... also ignore props in the world and go right through them."; ped side fixed here,
NPC skater side planned below.)

**Cause.** Ped navigation (M3) only knew the static NavPower navmesh and the other peds. The props (#15's dynamic
props, the DMOs) live in the physics' prop layer and were never handed to the peds, so a path or a step could run
through a bin, bench or barrier.

**Evidence (retail, [code], TU3 recompilation as reference).**
- Every world object carries an obstacle interface: register `sub_82595140` (entered through the base slot
  `sub_82595010` at vtable + 4) and update `sub_82595298`. Slot +68 says "I am an obstacle": 1 for the
  DynamicObject class (vtable `0x82322D50`, next to the string `dynamicobject` at `0x82322D2C`), for the player
  skater (`0x82300910`) and one more class (`0x823220E0`); 0 for peds (`0x8232BE80`) and the other actors.
- A DynamicObject takes its record from `DynamicObject Obstacle MemStore` (slot +76 `sub_82C57268`, record
  `sub_82C46778`, vtable `0x82322D10`). The box is its collision body's (slot +96 `sub_82C486F8` -> body vfunc 160
  half extents; slot +92 `sub_82C48688` orientation). Slot +88 `sub_82C48648` turns the obstacle off while the
  object's state word (`+144` component, `+4252`) is 1 (meaning not decoded; we use "carried").
- `sub_82C477B0` (each update): half extents below 0.2 m become 0.2 (`0x82099280`); faster than 0.4 m/s
  (`0x82181B90`, slot +24 = velocity) the cut is removed and NavPower's moving avoider (`+64`, `sub_82E99998`)
  takes over; at rest the box is cut into the NavPower mesh (`sub_8293EF28` with `{-1, 15.0, type 4}`, then
  `sub_8293F340`) and re-cut only after it moved more than 0.25 (`0x820C6D98`) x its smallest half extent.
  NavPower plans every ped bot on the cut mesh (its planner reflects `obstacles` and `dynAreas`), so retail peds
  walk round resting props, a moved prop counts where it lies, a carried or rolling one is not in the mesh.
- The PEDCOLL session (`pedcoll_20261004_215239`) logs only ped vs skater reactions (6 lines); it holds no prop
  data, so it validates nothing here.

**Change.**
- `skate-core::living_world::peds::obstacles` (new): `ObstacleParams` (retail defaults: on, 0.2 m, 0.4 m/s,
  0.25; ours: detour margin 0.1 m, max 8 detour corners, step height 0), `ObstacleInput`, `Footprint` (the box
  projected onto the ground as an oriented rectangle plus its height span), `NavObstacles` (stable ids in a
  `BTreeMap`, retail's cut / re-cut / moving / carried rules in `update`, a 4 m grid rebuilt only when a cut
  changes, `version`; queries `blocked`, `first_hit`, `detour`, `step_ok`, `resolve_step`).
- `peds::wander`: `PedNav::step_avoiding` (the old `step` = no obstacles): a probed target inside a cut does not
  fit (retail: no snap on the cut mesh), each new path gets detour corners round the cuts it crosses (the
  cheapest free corner of the grown rectangle on the mesh), and when `version` changes the rest of the path is
  checked again. `probe_fan_clear` / `choose_target_clear`.
- `skate-game::living_world::peds`: resource `PedObstacles`, system `update_ped_obstacles` (FixedUpdate, before
  `advance_peds`) feeds every prop body (`PropDynamics::obstacle_boxes`: box centre, basis, contact half
  extents, velocity, carried) and every mod body (`modding::bridge::obstacle_solids`: world AABB, the attached
  body carried; ids above 2^40); `advance_peds` steps with `step_avoiding` and never steps into a body
  (`resolve_step` slides along its face or the ped stays, then re-plans after 3 s as before).
- Open (2026-10-07): user report "Moving objects does not update the collision for peds, untested on skater
  npc's". The sessions had no obstacle lines, so logging came first: `PED_OBSTACLE` (a prop more than 0.1 m from
  where it was first seen: spawn, now, cut and cut centre, speed, moving, carried, footprint, version; on every
  change), `PED_OBSTACLES` (inputs, props, cut, moving, carried, moved; every 2 s with a moved prop, else 10 s) and
  `PED_BLOCKED` (a refused ped step into a body, or within 3 m of a moved prop's spawn spot; once per ped per
  second). Core helper `NavObstacles::blocker_at`. Session 2026-10-07 22:57 answered it: 7 of 8 moved props fell
  through the floor once released (13 m to 8-10 m at 7-9 m/s) and never rested, so they were never cut; the one
  that stayed on the floor was re-cut at its new spot (moved 1.12 m, speed 0) and the user saw "the peds stopped
  and then pathed around it". The ped side works; the cause is props sinking (open, prop dynamics).
- Moddable: `LivingWorldSettings::ped_obstacles`, Lua `sdk.world.set_tuning('living_world', {ped_obstacles =
  {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height}})`, readable via
  `world_tuning:living_world`, reset to retail on mod disable. Mod-spawned bodies are obstacles like props.
- Multiplayer: the obstacle state is plain data keyed by stable ids, updated once per population tick in id
  order; ped steps stay a function of records, tick, mesh and the obstacle list.

**Files.** `crates/skate-core/src/living_world/peds/{obstacles,obstacle_tests}.rs` (new), `peds/{mod,wander}.rs`,
`crates/skate-game/src/living_world/{peds,peds_tests,mod}.rs`, `crates/skate-game/src/physics/prop_dynamics.rs`
(`obstacle_boxes`), `crates/skate-game/src/modding/{bridge,world_tuning}.rs`, `crates/skate-mods/src/world_tuning.rs`.

**Verification.** skate-core `living_world::peds`: 29 pass (6 new: retail cut rules incl. 0.2 m minimum, 0.4 m/s,
0.04 m keeps / 0.06 m re-cuts, carried and removed, no new version while props rest; footprint orientation and
height; a ped walks round a bin and reaches its target, the same walk without obstacles crosses the bin; a prop
kicked onto the path at 2 s counts where it lies; 6 wandering peds among 12 props never inside one and identical
twice; step check lets a ped out of a prop pushed onto it). skate-game `living_world` + `physics::prop`: 62 pass
(new: 4 peds wander the plaza among 12 bins and a bench, never inside one, identical twice; obstacle inputs from
mod bodies and the Lua patch set / reject / reset). skate-mods `world_tuning`: 3 pass. Not seen in game yet.

**Open.** NavPower's moving avoider (`+64`) is not ported: a rolling prop is only solid for the step check. The
record's `-1` / 15.0 (all-movers blockage flags / penalty x15?) are not decoded; cuts block. The meaning of the
state word that switches the obstacle off is inferred. NavPower cuts polygons; we bend paths at rectangle corners.
The funnel can return a first corner behind a ped spawned within 1.5 m of the mesh edge (seen in a test, M3 code,
not changed here).

### NPC skaters riding through props, 2026-10-05 (fix 19)

**Problem.** User: "They also ignore props in the world and go right through them." (NPC skaters; peds were fixed
above.)

**Root cause.** Our NPC skaters are replay puppets. Their kinematic proxies (body capsule + board) joined the
player's contact solve, so the player bumps them, but not the dynamic prop step: `GamePhysics::step_props` got only
the local skater's board and skeleton volumes (`physics/solve.rs`), so no prop ever felt an NPC.

**Evidence (retail).** [code] NPC skaters are full skaters: an `AIController` drives the player's skater class
(AI skater vtable `0x82306320` has the player's slots, notes `npc-skaters-re.md` section 0), so their board and
body hit a DynamicObject (`0x82322D50`) like the player's. The AI controller also has an `ObstacleAvoider`
(controller `+80`, ctor `sub_824658C0`, gatherers `sub_82463C08` / `sub_82464000` / `sub_82464448` /
`sub_82464968` adding through `sub_82463A40`, mode pick `sub_82465578`, read by the PathController update
`sub_8246D560`); which object kinds it scans and what its modes do is not decoded, so it is not ported.

**Change.** The NPC's body capsule and board (as a capsule along the deck), at the recorded velocity, are push
volumes in the prop step, by the player's contact rule and each prop's own tuning (`props` domain: push mass,
transfer, caps). Both count as a board hit (the prop's `board_push_speed` cap): the lower body-bump cap is for the
player's walking body, which the prop's triangles stop; a puppet is not stopped, and a retail NPC rides into the
prop with its whole skater at riding speed. A bin on a line is knocked ahead of the NPC; a bench is shoved out of the
line (weight-scaled). The NPC itself does not slow down or bail (puppet; see open items). Each pushed prop records
the stable actor id that last pushed it (`PropDynamics::pushed_by`: 0 = local player, NPC = its proxy solid id),
the authority owner a future host uses; the push order is the local player first, then NPCs by id
(deterministic). Moddable: `sdk.world.set_tuning('living_world', {npc_skater_props = {enabled = false}})`,
retail on, restored on mod disable; push strength per prop type stays in the `props` domain.

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs` (`step_with_actors`, `LOCAL_PUSHER`, `pushed_by`),
`crates/skate-game/src/physics.rs` (`GamePhysics::actor_prop_volumes`, `step_props`),
`crates/skate-game/src/living_world/npc_skaters.rs` (`NpcSkaterPropContact`, `prop_volumes`, `push_proxies` fills
the volumes each tick), `living_world/mod.rs` (setting), `modding/world_tuning.rs` (apply + inspect),
`crates/skate-mods/src/world_tuning.rs` (`NpcSkaterPropsPatch`), `skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.** Seeded headless tests: `npc_skater_knocks_a_bin_on_its_line_out_of_the_way` (control run without
NPC volumes leaves the bin still; with them the bin ends more than 1 m ahead, never behind the board, owned by the
NPC), `npc_skater_bench_push_is_deterministic` (two runs identical bit for bit, bench pushed along),
`far_npc_leaves_props_alone_and_local_push_owns_it`, `living_world_npc_skater_prop_volumes_and_mod_switch`.
Not checked in game yet.

**Open.** (1) The ObstacleAvoider (slow down, stop or steer for an entry ahead) needs decoding first: trace hooks on
`sub_82463A40` (object added, its vtable = class) and `sub_82465578` (mode `+6000`, speed `+5996`), then an
avoidance input on the replay cursor. (2) Retail NPCs can bail on a heavy prop; our puppet bulldozes it instead.
Full report: `.local/research/npc/fix19-skater-props.md`.

## Peds walking through a held prop, 2026-10-08

**Problem.** User, verbatim: "Moving objects does not update the collision for peds, untested on skater npc's",
then (2026-10-08) "I wasn't recreating the issue last night, that may have just been while I was holding the
object, which is still wrong". So peds walk through a prop while the player holds it with Move Object.

**Cause.** The port treated "held" as retail's obstacle-off gate (the state word below), so a held prop was
neither cut into the walkable area nor solid for a ped's step. The retail code says otherwise.

**Evidence (retail, [code], TU3 recompilation as reference).**
- The obstacle-off gate is DynamicObject slot +88 `sub_82C48648`: off while the component at `+144` reports
  `+4252 == 1` (component vtable `0x82322ED8` slot 31 `sub_82C56AE8`). That word is set to 1 only by component
  slot 32 `sub_82C56B00`, which also moves every collision element of the object to collision group 13; slots 33
  `sub_82C56BA0` / 34 `sub_82C56C70` set it back to 0 (group 12 or 14 by a per-type value). No direct caller of
  slot 32 was found; what state 1 means is still not decoded.
- The Move Object hold does not touch it. Each held tick the state calls interface slot 10 (keep-alive, type 2
  record), flushed to the DMO handler's slot 6 `sub_82C4C450`, which calls component slot 30 `sub_82C485D0` with 1:
  that only sets the held bit `DMO+4464 & 0x20` (getter slot 29 `sub_82C485C0`). The obstacle update
  (`sub_82595298` -> `sub_82C477B0`) never reads that bit; its "force moving" input is DMO slot +104 = 0.
- So in retail a held prop stays an obstacle: while it moves faster than 0.4 m/s (`0x82181B90`) its cut is
  removed and NavPower's moving avoider takes over (`sub_82E99998`: an 88-byte record in the planner's obstacle
  database with position, velocity and a radius of 0.35 x a planner-wide value, independent of the box; moved every
  tick by `sub_82E998C8`, removed by `sub_82E99BB0`); held still (or slower) it is cut where it lies, and re-cut
  after it moved more than 0.25 x its smallest half extent. After release the same rule cuts it where it rests.
- How NavPower's bots steer round a moving avoider is middleware internals (the database is only reached through
  generic query functions in `0x82EA8000..0x82EAC000` from NavPower code, no Skate-side reader); not decoded.

**Change.**
- `skate-core::living_world::peds::obstacles`: `ObstacleInput::held` (separate from `inactive`, which stays the
  retail off word), `ObstacleState::held`, `ObstacleParams::held_is_obstacle` (retail true: a held prop or an
  attached mod body stays an obstacle; false = the earlier "held is ignored" rule, mod option).
- `ObstacleParams::moving_solid` (default true), NOT RETAIL YET: stands in for the NavPower moving avoider. A
  moving object is solid for a ped's step (`step_ok` / `resolve_step`: slide along its face or stay, then re-plan),
  it does not bend paths. False = moving objects do not block a ped's step; this is the switch a decoded avoider
  port replaces.
- `skate-game::living_world::peds`: `prop_obstacle_inputs` (props: held = `PropDynamics::held`, never inactive);
  mod bodies: the attached body is held, not inactive.
- Logging: `PED_OBSTACLE` now prints `held`, `off` (the retail off word), `role` (`cut`, `solid`, `none`, `off`)
  and `cut_off` (distance from the cut centre to the body centre, -1 without a cut), on every change of cut /
  moving / held / off, on a re-cut, and once a second while held, so a dragged prop's body pose shows next to its
  cut pose. `PED_OBSTACLES` adds `off=` next to `held=`. `PED_BLOCKED by=` names a held prop that refused a step.
- Moddable: `ped_obstacles {held_is_obstacle, moving_solid}` in the `living_world` tuning (Lua
  `sdk.world.set_tuning`), readable via `world_tuning:living_world`, reset on mod disable.
- Multiplayer: plain data by stable id; the held flag comes from the authority's carry state.

**NPC skaters and moved props.** Retail AI skaters are full skaters: their board and body hit a DynamicObject
by the same rigid-body contact as the player (fix 19 ported this: our puppets push props, a moved prop is pushed
where it lies now). Their `AIController` also runs an `ObstacleAvoider` (controller +80) with four gatherers;
2026-10-08 they read three pools of one world container (globals `0x8308549C` = container +16 for the 8 m
gatherer `sub_82463C08`, `0x830854A0` = +2704 for the 20 m `sub_82464000`, `0x830854A8` = +22416 for the 16 m
`sub_82464448`; filled by `sub_826BBC40`); which object kinds those pools hold and what the avoider's modes do is
NOT decoded, so a moved prop does not change an NPC skater's line in our port (NOT RETAIL YET; no speculative
slow-down added).

**Files.** `crates/skate-core/src/living_world/peds/{obstacles,obstacle_tests}.rs`,
`crates/skate-game/src/living_world/{peds,peds_tests}.rs`, `crates/skate-game/src/physics/prop_dynamics.rs`
(test), `crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`, `sdk/skate.lua`.

**Verification.**
- New test `physics::prop_dynamics::tests::peds_do_not_walk_through_a_held_prop_and_see_it_where_it_rests`
  (headless, real Move Object carry path on the street fixture): a bin (half 0.35 x 0.5 x 0.35) is dragged
  toward a ped walking head-on along its line for 1 s, held still 3 s, released and left 3 s; obstacles come from
  `obstacle_boxes` through the game's input mapping each tick, the ped steps with `resolve_step`. Retail rule:
  0 ped steps deeper into the prop (past the 0.25 x 0.35 m re-cut tolerance); control (`held_is_obstacle = false`,
  the earlier rule): 8. Held and moving faster than 0.4 m/s for 65 ticks (solid), held and cut for 173 ticks.
  After release the bin rests 2.94 m from its spawn, cut at its new spot (cut centre 0.022 m from the body,
  inside the 0.0875 m re-cut tolerance), obstacle version 3; the new spot blocks and a path leg through it hits the
  cut, the old spot is free and a leg through it does not.
- New core test `living_world::peds::obstacle_tests::held_prop_stays_an_obstacle` (held still = cut, dragged at
  1 m/s = no cut but solid, `moving_solid = false` and `held_is_obstacle = false` switches).
- `living_world_ped_obstacle_inputs_and_mod_tuning`: the attached mod body is held (not off); the Lua patch sets
  `held_is_obstacle` / `moving_solid` and mod disable restores the defaults.
- Suites (`--release --locked -j 4`): skate-game `living_world physics::prop modding::world_tuning` 108 passed,
  3 ignored (asset tests); skate-core `living_world` 128 passed; skate-mods `world_tuning` 3 passed. Not run in
  game.

**Open questions.** NavPower's moving avoider (how bots steer round it) is not ported; ours blocks the ped's step
instead. The meaning of the off word (state 1, collision group 13) is not decoded. The NPC skater
`ObstacleAvoider` pools and modes are not decoded. Not seen in game yet: drag a prop into a ped's walk in DownTown
and check the `PED_OBSTACLE` lines (`held=true role=solid` while dragging, `role=cut` held still and after it rests).

## Skater hits peds: knock-down or stumble (2026-10-08)

**Problem.** Skating into a ped did nothing. Retail peds fall over (animated knock-down, then get up) or stumble.

**What retail does** [code, TU3; recomp disassembly as reference; `.local/research/peds/skater-ped-contact.md`]: the ped
contact callback `sub_82E38FB8`, kind 5 (an `IActor` toucher such as the skater):
- inputs: the length of the ped body's linear and angular velocity after the contact solve, gated by entity
  `C04236FB548697D0` / `797AA1D5F828819B` (0 for every stock entity);
- knock-down when the ped's animation set allows it (field `5B92564B352A9FAA`, off for the four marquee sets) and a
  speed is above `541FFA2E9D81C947` / `7C5E39ECE5A5572E` (3.0 / 3.0); otherwise a standing stumble unless the brain's
  `+3277` bit 0x10 makes it immune. The 6.0 thresholds and the "no reaction" cases belong to a ped touching a ped
  (toucher cast to `IPedestrian`, `sub_826C2C70`);
- direction: the contact normal (flat) against the ped's forward, `acos`, negated when `cross(forward, normal).y > 0`,
  in degrees: FromBack within 35, FromLeft 35..145, FromRight -145..-35, FromFront beyond (constants `0x8206D148`,
  `0x822F9428..30`; names from the table `0x830218B0`);
- reaction [data, `motiongraph_collision.xml`, `template/knockdown.xml`]: a stumble plays CollisionBack / Fwd /
  LeftStanding (FromLeft mirrored, 0.3 s blend); a knock-down plays WipeoutBack / Fwd / LeftFall (0.4 s), the ground
  cycle until the AI's `Recover`, then the get-up (0.1 s). Ground time: animation-set field `AD3C483F0C9DAD67` (1.5 s,
  security 0.5 s), read by `sub_8269A990` in `PedestrianColliding`'s update.

**Change.** `skate_core::living_world::peds::skater_contact` (decision, direction, reaction steps);
`PedAnimPlayer::react` and the `Reaction` locomotion state (steps in order, the ground cycle for the ground time, then
idle; no navigation meanwhile); `PedAnimSet.collision` filled from the export; `advance_peds` checks every observer
against every ped, logs `PED_SKATER_CONTACT` and sends `PedEvent::Hit` (mods: `on_event` `ped_hit`).
NOT RETAIL YET: the skater is a 0.35 m cylinder at the observer (2 m tall) and the ped body's post-contact speed is
the skater's speed into the ped (no Havok ped body); the normal is taken as pointing into the ped; where the ground
timer starts is not pinned (here: on reaching the ground cycle); the ped is not pushed aside.

**Verification.** skate-core: 3 `skater_contact` tests (buckets, 3.0 rule, marquee, immune, steps) and
`a_knock_down_falls_lies_for_the_ground_time_gets_up_and_walks_on` (90 frames on the ground at 60 Hz). Data-gated on
the user's export: default 3.0 / 3.0, allowed, 1.5 s; marquee not allowed; security 0.5 s; all 12 reaction animations
resolve to clips in the ped bank. skate-game `living_world_a_skater_knocks_a_ped_down_or_makes_it_stumble` (6 m/s
knock-down, 1 m/s stumble, one reaction, back to locomotion), `living_world_messages_become_mod_events` (`ped_hit`).
Not play-tested yet.

## Ped behaviour runtime: the stock ped AI graph on each ped (2026-10-09)

**Problem.** Peds only wandered and reacted to contact; retail peds also warn, flee, observe, chase and take
the skater down. That behaviour is mostly data: the stock ped AI graph and the mood tables.

**Retail [code] [data] (`.local/research/peds/b1-ped-behaviour-runtime.md` to `b5-ped-mood-events.md`; main
checked the key addresses).** Retail runs the ped AI graph on the same dynamic controller as the skater's graphs
(vtable `0x82321EA8`, ctor `82C12E88`, created through `82C12DD0`); every ped condition goes through the masked
activation gate `82C12D48`; behaviour slots 13 / 14 / 15 are Begin / Update / End. The shipped
`Pedestrian.stategraph` is already compiled (includes and templates expanded). The ped brain holds wants
(`brain+412+want*12`: target, flag 0x80 "needs addressing"), timers (`82E40940` / `82E40A80`), the honker
(`+3232`) and the scatter bit (`+3281` 0x80). Wants come from the mood system: per-ped mood records (presence of a
player within 35 m every 0.5 s, collisions, tricks; `sub_82E41060`, at most 10 records) and the producer
`sub_82E41B98` that rolls the ped type's moodresults (`rand() % 100 + 1` against the probability,
`sub_8269A588`).

**Change (step 1 of the runtime).**
- skate-core `living_world::peds::brain`: `PedBrain` (wants, timers, flags, outputs) and `BrainHost`, the
  graph host. Ported operations: `HasSpecificWantThatNeedsToBeAddressed`, `NeedsToBeginColliding`,
  `IsPedestrianColliding`, `ShouldScatter`, `IsBeingHonkedAt`, `IsZombieMode`, `HasPlugin`,
  `DistanceToWantTarget` (squared compare), `Wander` (`8269F380` / `8269F410`: motion intent 6, 2.0 m/s),
  `NoRoadWander` (`826A2FB8`), `SuggestVelocity`, `KnowAboutWantTarget` (30.0), `UnsetWant`, `UnsetWantOnEnd`,
  `ChannelWarnWantTarget` (`8269F7B0` / `8269F980`: warn timer 36 = 3.5 s, `0x82063AB8`, then the want goes),
  `Flee` (`826A3530` / `826A3760`: motion intent 4, the flee target), markers. Every other name is `Pending`:
  false / no effect, listed at load (`PED_GRAPH loaded: ... not ported yet: ...`).
- skate-core `living_world::peds::mood`: the mood record store (post / bump / at most 10 / tick / expiry) and the
  presence scan (35 m). Not wired yet: the field binding for magnitudes and lifetimes is still open.
- skate-game `living_world::ped_graph` loads the stock ped AI graph with the ped data; `think_peds` runs each
  ped's graph once per population tick before the bodies step (host only), logging `PED_BRAIN` on every state
  change. Mod values `ped_brain {enabled, wander_speed, warn_seconds, know_about_seconds}`.

**What changes in game now.** Nothing visible yet: no want is produced until the mood system is wired, so every
ped runs the graph into its wander state (`NoRoad`) and the body keeps its current navigation.

**Verification.** skate-core `brain` tests (4) and `mood` tests (3). Data-gated
`living_world_ped_ai_graph_runs_on_the_brain`: the stock graph loads, binds and compiles (202 behaviours, 149
conditions); an idle ped reaches `NoRoad` with the wander intent; a flee want takes it to `Fleeing` (intent 4,
fleeing from the target); once the target is more than 15 m away it releases the want and wanders again.
Muted DownTown run (90 s): `PED_GRAPH loaded: 202 behaviours, 149 conditions; 115 operation names not ported yet`; all 36
peds went `InitialState -> NoRoad` with the wander intent; no panic or error.

**Open.** The flee movement (`826A35F8` is a chain of navmesh calls with 1/6, `0x822F8604`; until it is decoded
the body does not run away), the mood field binding and the want producer, the rest of the operations (chase,
takedown, taze, observe, speech events, crosswalk and road ops), the ped motion graph's own operations.

## Ped mood system: wants from the stock mood tables (2026-10-09)

**Retail [code] [data] (`.local/research/peds/b4-ped-want-producer.md`, `b5-ped-mood-events.md`,
`b6-ped-mood-fields.md`; main checked the producer call sites, the roll, the presence constants, the magnitude
getter hash and its fallback).** Peds keep mood records (`brain+1040`, at most 10, keyed by instigator, second
entity and category): presence of a player within 35 m posts every 0.5 s (`sub_82E3CA20`, category field `B1DD`,
which is also the per-post magnitude), a collision posts `collision` to the hit ped and `nearbycollision` to the
others (`sub_82E41060` rewrites the category when the "ped itself" prerequisite fails). Records age and expire
after the category's lifetime (`A379`, 30 s); magnitude and count never decay. The producer (`sub_82E41B98`,
evaluator `sub_82E41EB0`) takes the ped type's reaction results by priority and checks, in order: target,
category, instigator type, magnitude (`C404`/`8239`), Nth bump (`B1A9`/`0982`), second-entity type, hand prop,
two target checks, outstanding reactions (`7FF7`), cooldown, prerequisites, no pending want; the winner is rolled
(`rand() % 100 + 1`), every roll starts its cooldown, and a pass raises its wants on the target and suppresses
the record (`4EC2`). Discrete categories have no `B1DD`: the attribute default at `0x830D0850` is 0.0 (the field
research named 0x820D0850 and 3.5e13; corrected).

**Change.** skate-core `living_world::peds::mood`: tables, store, presence scan, prerequisites and the producer,
all values from the tables. skate-game `living_world::ped_mood` reads them from setup data (`tables.json`, parents
resolved; each entity type's reaction set by field `712E`); `think_peds` ticks each ped's records, posts presence
and the skater collisions (`PedEvent::Hit`), runs the producer with a per-ped seeded RNG and raises the wants on
the ped's brain before its graph runs. Log `PED_MOOD` (result, category, roll, wants), `PedEvent::Mood`; mod switch
`ped_brain.mood` (setting, mod patch later).

**Engine choices / inferred.** Prerequisite kind 3 measures to the instigator, kind 6 asks whether the second
entity is the ped (consistent with the collision rewrite); the undecoded target checks, hand-prop bit and
ped-state prerequisites answer false; the outstanding count is the number of raised wants about the instigator;
the sight test is not applied; collisions name the first player as instigator; the evicted record when full is
the oldest.

**Verification.** skate-core `mood` tests (4). Data-gated `living_world_ped_mood_tables_warn_then_chase_on_collisions`
on the stock tables (27 categories, 88 results, 17 reaction sets): an adult male hit by the skater does nothing
on the first hit, raises `warn` on the second (`skatercollisionwarn`, Nth 2) and `angrychase` on the third
(`skatercollisionchase`, Nth 3); a bystander gets neither.
Muted DownTown run (120 s, the player standing at the spawn): `PED_MOOD tables: 27 categories, 88 results, 17
reaction sets, 110 entity types`, 45 peds wandering, no reaction (a default ped's presence results need about 10 s
within 12 m or a hand prop; the rest are ped-to-ped greets, whose presence posting between peds is not done yet).

**Warning stops the ped and faces the skater.** Ported from `.local/research/peds/b7-ped-reaction-movement.md`
(main checked the constants): `StopAndFaceWantTarget` (Begin saves the speed suggestion; Update faces the want
target flat, speed 0.0 `0x82165A10`; End restores), `WatchWantTargetWithoutInterruptingLocomotion` (keeps walking
while the target is within 55 deg of the facing, `0x821DBCF4`, else latches into stop and face until End),
`TurnToFaceSkater`, `StandAndWatchSkater` (watch point 4 m ahead, `0x82257308`; jumps to the skater's position +
velocity x `predictTime` 2.0 when the directions differ, dot < `maxAngle` 0.4), `IsFacingSkater` (dot > `FOVAngle`).
The body stands and turns toward the face point while the speed suggestion is 0 (turn rate: our nav's 45 deg/s
until the motion graph's turn branches run). Data-gated graph test: a warn want takes the ped to
`PedestrianDoWarning`, speed 0, facing the skater. So in game: a ped hit twice by the skater stops and faces them
for 3.5 s; hit a third time it raises a chase (chase movement not ported yet).

**Open.** Flee movement (the steering goal provider, research running), chase movement, speech events, the
undecoded checks above, presence posting between peds (greets, conversations), other posters (trick, slam, chase,
noise, greeted).

## Fleeing peds run away from the threat (2026-10-09)

**Retail [code] (`.local/research/peds/b8-ped-flee-steering.md`; main checked the 15 m leg `0x820BD16C`, the 1 m/s
speed gate `0x8231A844` and the 0.5 blend `0x8209975C` in `sub_82E318F8`).** The Flee intent installs a goal
provider (`sub_82E35E80`) with a 2.0 m arrival radius. Each leg goes 15 m from the ped: straight away from the
threat when the threat moves slower than 1 m/s or the ped is behind its motion, else `0.5 x (side + away)` (side =
the threat's motion x up, on the ped's side; not renormalised), then the navmesh goal test (not a cast, b650). A new leg starts when the ped
is within 2 m of its goal (`sub_82E2BC70`). The speed is the ped type's chase record field `CD65` (11 for
pedestrians), sent to the motion graph as a run gait (`sub_82E2D260`).

**Change.** skate-core `living_world::peds::flee` (direction, goal, arrival; `FleeParams` with the retail values);
the ped animation gains the run gait (`Intent::Run`, `Locomotion::Run`, logical clip `FwdChaseRunCyc`, the stock
`*_CHASE_RUN_N_0_CYC`, hash `7746273E01422734`). `think_peds` turns a fleeing brain into nav legs (a one-point
route per leg, cleared when the flee ends; the goal test and rewrite as in "Flee legs go round rails and walls"
below), logged `PED_FLEE`; `advance_peds` runs where the nav walks while fleeing. Settings
`ped_brain.flee` (retail values).

**Engine choices.** The run's own start, stop and turn clips are not wired (it enters from standing or walking and
stops through the walk stop) until the motion graph runs; the speed is the run clip's root motion, not the chase
field; a mod route on a fleeing ped is replaced by the flee legs and cleared after.

**Verification.** skate-core `flee` tests (2: straight away 15 m with arrival at 2 m; blended to the side for a
threat coming at the ped, straight away from behind it). Full workspace run: only the 4 known pre-existing
failures. Not seen in a game run yet (a ped flees only on a raised flee want; for default peds that comes from the
female reaction set's `fleeperp` after repeated hits).

**Open.** Which stock reactions raise `flee` for which ped types in practice (play-test), the run start / stop
clips. (The blocked-goal rewrite: see the next section.)

### Flee legs go round rails and walls (2026-10-27, ask 393)

**Problem.** Fleeing peds ran into rails and walls and ran in place (session game-20261027-101012). The user: "peds
fleeing still will run into rails and walls and get stuck trying to flee on a path that isn't traversible."

**Root cause** (research b650, `.local/research/b650-ped-nav-failures.md`; main checked the code lines). Retail
[code]: the leg goal test `82C465A8` is not a line cast but the chase reachability test (ped and goal on the navmesh,
connected); a reachable goal stays even with a wall between, and the NavPower path goes round. Only an unreachable goal
is rewritten (`82C46208` -> `82933E18`) to the nearest navmesh point reachable from the ped's polygon (unbounded
range). Ours cut the 15 m leg at the first wall or rail along a straight line (8-step bisection of `clear_line`): the
cut goal often lay within the 2 m arrival radius, so a new leg was planned every tick and the ped ran in place.

**Change.** skate-core `flee::goal_on_mesh` (reach test via `chase::reach_failed`, rewrite via
`NavMesh::locate_nearest`, the existing port of `82933E18`; no result keeps the raw goal, retail code 3); `think_peds`
uses it instead of the bisection. Mod keys `ped_brain.flee_leg`, `flee_arrival_radius`, `flee_threat_speed_gate`,
`flee_side_blend` (doc 26a). The path itself stays the wander planner's `find_path` + obstacle detour (our NavPower
stand-in).

**Open.** Retail's 0.5 m snap window and the poly class test `82C47D60` in the reach test (shared with the chase
check, ask 392); a cornered ped (goal within 2 m of the reachable edge) re-plans every update in retail too, and
whether it then stands or jogs in place is not checked.

**Verification.** skate-core `a_flee_goal_stays_behind_a_wall_and_moves_off_an_island`; skate-mods
`valid_patch` asserts for the new keys. Not play-checked.

### Flee re-leg when the path ends (2026-10-29, ask 430)
Problem: a ped fleeing a pushed bench was blocked by the player 6.5 m short of its leg goal and stood there 45 s in Fleeing (session 2026-10-29, ped #163); ours asked for a new leg only on arrival within 2 m.
Retail [code, research b697, main re-read `82E2CB08` in the recomp]: besides the arrival test (`82E2BC70`), the mover's per-tick update `82E2CB08` asks the goal provider for a new goal when the bot is done (`82E96728`: no path, or the path's end reached) and the mover's vfn 0xA0 returns 0; for Flee that is the flee goal vfn `82E31818`. There is no timer and no blocked-by-a-body rule; the 4-corner steer loop in `82E2C0C8` is the vehicle avoidance modifier, not used here.
Ours: `PedNav::route_done` (skate-core `peds/wander.rs`) is set when a planned route's path ended or failed; `think_peds` gives a new leg on arrival or `route_done`. Mod key `ped_brain.flee_releg_on_nav_done` (retail true, doc 26a).
Open: a ped pinned by a standing or moving skater with its path unfinished still presses on: how retail's walking path controller (`82EA0808`) steers round NavPower moving avoiders is not decoded (b697 open 1; next research b698).

## Peds speak from their AI graph (2026-10-09)

**Retail [code] (`.local/research/peds/b9-ped-speech-events.md`; main checked every address below in the TU3
recomp).** A graph speech value is not a sound request. `SendSpeechEvent` Begin (`sub_826A3508`) calls ped vfunc
+204 with the op's `speechvalue` (float attribute truncated to an int; `speechevent` is only a label); that virtual
(`sub_82E22798`) stores it at `ped+2468`. The ped constructor (`sub_82E33198`) starts the field at 68, which no line
uses. `ChannelWarnWantTarget` Begin (`sub_8269F7B0`) sends 53 through the same virtual (54 when the ped's `+2000`
component answers non-zero; that query is open). The audio side's PedestrianSpeech asks for a line only when the
value changes, so a state re-sending the same value stays silent. `SendChaseStateMessage` and
`EmotionalResponseToGivingWarning` post game messages and do not speak.

**Change.** skate-core brain: `PedOp::SendSpeech { value }`, `PedBrain::speech` (the `ped+2468` value, `None` = the
constructor's 68), the warn op writes `BrainSettings::warn_speech` (53). `think_peds` sends a changed value as
`PedSpeechEvent` to the audio side (which already ports value -> line, cooldowns and the pick), logs `PED_SPEECH`
and posts `PedEvent::Speech`. Mods: `ped_brain.warn_speech` (0..=127) and the living-world event `ped_speech` (`id`,
`value`, `state`).

**Engine choices.** Only a change is sent, so the audio bridge's "repeat through 0" rule (kept for the Lua `speech`
call) never fires for graph speech. The listener-side gating (15 nearest peds within 50 m) stays in the audio port.

**Verification.** skate-core `speech_ops_store_the_value_on_the_ped` (truncation, default 0, Begin only, warn 53);
skate-mods patch validation for `warn_speech`; `living_world_messages_become_mod_events` covers `ped_speech`.
skate-game living-world tests: 67 passed. Not heard in a game run yet.

**Open.** The `ped+2000` query (53 vs 54); value 55 (most likely a speech value from StartChase Update `826A3780`, inferred) has no mapping in our
speech manager; value 15 from NewChasee (`826A37F0`) maps to 607_chase_join and arrives with the chase port.

### Graph timers: SetSimpleTimer and SimpleTimerExpired (2026-10-09)

**Retail [code] (main read these in the TU3 recomp).** `SetSimpleTimer` (factory `826C9E88`, Begin `826A2810`) and
`SimpleTimerExpired` (factory `826CFA50`, condition `826ACFF0`) read `timerName` (default "TimedRangeRandom") and
`length` (default 0.0). The name maps to one of 47 timer indices (`sub_82E42B08`, e.g. 21 StartChase, 42
TargetUnreachableTimer; unknown name = 47). The brain's timer map (`sub_82E40940`) removes a timer set to 0 or less
and drops a new timer when 14 are running; reading a timer that is not running gives 0 (`sub_82E40A80`), so it
counts as expired. These ops are used 13 and 16 times in the stock graph (chase hold, alert, sit, wander waits).

**Change.** skate-core brain: `timers::NAMES` / `timers::index`, `PedBrain::set_timer` / `timer`, ops
`PedOp::SetSimpleTimer` and `PedOp::SimpleTimerExpired`; the warn op uses the same timer rules.

**Verification.** skate-core `simple_timers_follow_the_retail_timer_map`.

## Ped chases: chase record, chase groups and the intercept (2026-10-09)

**Retail [code] (`.local/research/peds/b10-ped-chase-takedown.md`, `b11-ped-intercept-takedown.md`,
`b12-ped-chase-group.md`; main checked the addresses named here).**
- Chase record: each ped type points at a `livingworld_entities_chase` record (entity field `FBC4...`; escape
  distance 65 m for pedestrians, 500 m security, run speed `CD65` 11 m/s, max lead time `47C9` 6 s, give up after
  `78E2` takedowns). The chase manager (`*0x830854B8`) holds the `global` record (predict angle `5D48` 22.5 deg,
  max chasers `CEA5` 5) and the "chases allowed" switch (`+6332` bit 0x10, on at init `826B41B0`).
- Chase group: every chasee (the player's actor and every ped) has a group of up to 25 chasers in join order; entry
  0 is the primary chaser (`82D97078`). Joining (`82D96C38`) refuses duplicates and full groups and resets the end
  reason on the first join; leaving the last chaser resets it too (`82D96E88`). `GiveUpBeingPrimaryChaser` swaps
  entry 0 with entry 1 and holds timer 29 at max(itself, `timeout`) (`82D97108`).
- Ops: `IsChasing` (`826AC470`), `ChaseeEscaped` (`826AC668`, 3D distance past the escape distance),
  `CanNewChaseStart` (`826AAC08`), `CanChaseeAddNewChaser` (`826ACAC8`), `IsPrimaryChaser`, `IsChaserEndingChase`
  (group reason, else the chaser's own), `ChaserShouldGiveUpDueToTakedowns` (`826AD690`), `NewChasee` (`826A37F0`:
  speech 15, takedowns 0, join the angrychase target's group), `StartChase` Update (speech 55), `ChaserEndChase` /
  `ChaserGroupEndChase` (reason: default 0, aggressivecapture 1, returntopatrolzone 2, lostinterest 3 via
  `sub_82BFEF30`, inferred), `EndChase` (`826A53F0`: leave, forget the chasee, steering speed 0).
  `ChaseeHasProtector` and `ChasersAreScared` answer false (only the actor has a protector interface, contents open;
  scared is never true in retail).
- Intercept (`826A3BF0` / solver `sub_82E3D4D8`): each tick, if the chasee is reachable, the flat intercept time
  `t` with `|Q + V t - P| = run speed x t`; no solution or `t` not below the max lead time gives no new goal. When
  the angle between (chaser - chasee) and the chasee's velocity is strictly between the predict angle and 180 deg
  minus it, the goal leads the chasee (`Q + V t`), else it is the chasee. Goal height = the higher of the two; nav
  speed = run speed, arrival 0.5 m.
- `ActivateRelatedWant` (`826A6210`): copies the original want's slot (`originalWant`) to `relatedWant` and flags
  it (`82E42A58(brain, want, 1)` writes the 0x80 needs-addressing bit); with `deactivateOriginalWant` (default
  true) the original's bit is cleared (joinchase -> angrychase). `ChangeNavModifierSetting` (factory `826A74C8`,
  Begin `826A75D8` saves the ped's setting and sets `set_to`, End `826A7658` restores it; modifiers ChaseGlue 0,
  Pedestrian 1, SkaterAvoidance 2, VehicleAvoidance 3).

**Change.** skate-core `living_world::peds::chase` (`ChaseRecord`, `ChaseGroup`, `intercept`, `escaped`, end
reasons); brain ops for all of the above with `ChaseRequest`s the host applies in order and a `ChaseView` (own id,
record, groups); `BrainSettings` gains `chases_enabled`, `start_chase_speech` (55), `new_chasee_speech` (15).
skate-game: `PedData.chase` / `chase_global` from `tables.json`, the host-owned `PedChaseGroups` resource keyed by
chasee id (chasers that leave the world leave their groups), `think_peds` applies the requests (log `PED_CHASE`,
`PedEvent::Chase` -> mod event `ped_chase`) and steers intercepting peds to the solver's goal with the run gait (log
`PED_INTERCEPT`). Brain ops `ActivateRelatedWant` and `ChangeNavModifierSetting` (`PedBrain::nav_modifier`); the
nav skips ped avoidance while the Pedestrian modifier is off.

**Engine choices.** The player's group reads the `global` record for its max chasers (the actor's own record is
not decoded). The solver takes the smallest positive root (`sub_82E15CD0`'s choice between two roots is not
decoded). The intercept's reachability and projection use our navmesh line and locate. GiveUpPrimary's trailing
reorder after the swap is not decoded. Nav modifiers default to on (per-type defaults open); only Pedestrian is
applied (ChaseGlue, SkaterAvoidance and VehicleAvoidance are kept but not used by our nav yet).

**Verification.** skate-core: `crossing_chasee_is_led_and_one_running_away_is_chased_directly`,
`record_fallbacks_and_escape`, `groups_keep_join_order_and_hand_over_the_primary`,
`chase_conditions_read_the_chasee_and_the_record`, `chase_group_ops_ask_the_host_and_read_the_group`; skate-game
`chase_records_merge_parents_per_entity_type` and the mod event test. Not seen in a game run yet; the takedown
itself (contact, success, the skater's bail) is not ported.

**Open.** Takedown (`b13-takedown-skater-side.md`: success sets a pending flag and direction on the player's actor;
the bail it causes is being researched), the takedown entry choice `82E3C000`, LostChasee / perception
(CanSeeChasee), RestFromChase, BlockChasee / PursueChasee, the protector, value 55's line.

## Ped takedowns and chase exhaustion (2026-10-09)

**Retail [code] (`.local/research/peds/b13` to `b17`; main checked the addresses named here and the setup data).**
- Choice (`82E3C000`, each tick from TakeDownTargetablePredictions): the ped type's `livingworld_entity_takedown`
  list (`generic_male` 7 entries) in priority order; approach side from `normalize(ped - target) . target
  velocity`; per entry the target `lead + 0.06` s ahead, at most 1.25 m above or below, distance from the reach point
  `ped + 0.2 x forward` inside `(min + 0.2 + 0.06 s, reach + 0.2 + 0.06 s]`, signed bearing inside the entry's
  angles (or mirrored). The first fit wins.
- Attempt (`pedestrian_attempttakedowntargetable.xml`): sustained by the ActiveTakedown intent, timer 16 = 3.0 s,
  speech 17; AttemptTakeDownTargetable Update (`826A4BE8`) succeeds when the ped touched its target during the
  takedown (the contact callback `82E38FB8` records the touch and counts it at `brain+3248`); success
  (`826A4E50`) calls the target's `vfn20(chaser position)`: on the player's actor (`82592390`) that sets `1904` bit
  30 and the direction away from the chaser (main checked; the actor always accepts, `8281DD70` returns 1). Speech
  65 for the player, else 19. Failure: speech 18.
- On the skater: the flag reaches the animation packet (P+10496, +10480; `82593640`), Processed +2468 bits 2 and 18
  (`82DB5BE0`, `82BDA0D0`); bit 18 is phys-out byte 59, which the stock motion graph reads (`IsPhysicsWiping`,
  `onboard.xml`) to enter WipeOut; WipeoutGround Enter (`82D3B5E8`) pushes skeleton part 12 along the direction
  scaled by the vault value `LivingWorldPushForce`.
- Exhaustion (`b17`): a float at `brain+3220` grows by the tick in InterceptChasee / PursueChasee; NeedToRest is
  `accumulator > exhaustion_limit` (`82E3D7B0`: peds 30 s) or resting and not rested (timer 4 still running).
  RestFromChase: accumulator 0, resting, timer 4 = rest time (`5960`, 5 s); End: not resting. ClearExhaustionTimer:
  accumulator 0. Investigate: timer 1 = investigate time (`1AE2`, peds 10 s), End sets 0, exceeded at 0.

**Change.** skate-core `living_world::peds::takedown` (`TakedownTable`, `choose`); brain ops
ChaserEvaluateWhoToTakeDown, TakeDownTargetablePredictions, HasTakeDownTargetable, CanAttemptTakeDownTargetable,
HasMonitoredIntent (ActiveTakedown), AttemptTakeDownTargetable, TakeDownAttemptSuccessful / Failed,
TakeDownTargetableSuccess / Failure, TakedownTargetableClearOnEnd, NeedToRest, RestFromChase, ClearExhaustionTimer,
Start / EndInvestigateTimer, InvestigateTimeExceeded; `ChaseRecord` exhaustion / rest / investigate getters.
skate-game: takedown tables from `tables.json`; the contact (our skater cylinder against the ped while the takedown
plays) sets the brain's contact and counts the takedown; a success writes `SkaterTakedownRequest` and
`apply_skater_takedowns` sets the skater's takedown latch, which the animation packet publishes as the external
impulse (flags 10496, vector 10480) until WipeoutGround Enter uses it. Logs `PED_TAKEDOWN`, `SKATER_TAKEDOWN`; mod
event `ped_takedown` (`id`, `target`, `success`).

**Engine choices.** No takedown clips play yet: the ped stands (SuggestVelocity 0) and the takedown intent ends
with the decision; the contact is our skater cylinder, not the ped body's Havok hit (`*(ped+2032)`, its installer
not found); the target never vetoes (`T.vfn24` byte +59 open); the latch drops after 2 s if no wipeout used it
(retail's clear of bit 30 not found); the ped's speed for the window is its position change.

**Verification.** skate-core `first_fitting_takedown_wins`, `a_takedown_attempt_succeeds_on_contact_with_the_target`,
`chasing_exhausts_and_resting_recovers`; skate-game data-gated
`a_ped_takedown_publishes_the_external_impulse_and_wipes_the_skater_out` (DownTown: our skater is in WipeoutGround
from the first tick after the latch, and the latch is used). Not play-tested.

**Open.** The takedown clips and the ped's own stumble on failure; the perception (CanSeeChasee, LostChasee) and
SetAltTargetToChaseePosition / mood suppression ops (research b18 running); Pursue / Block for secondary chasers.

## Ped perception, secondary chasers and tazers (2026-10-09)

**Retail [code] (`.local/research/peds/b17` to `b20`; main checked the setup data and the key addresses).**
- Perception (`b17`, `b18`): at most 5 entries per ped (`brain+624`), a new one evicts the oldest and starts "just
  told"; each tick memory and suppression run down, age up, the vision test (`82E27240`) sets "visible" and refreshes
  the last position: seen within 1 m (2 m height), or within the near range (20 m x the type's `D036`), or within the
  far range (30 m x `D238`) inside the 120 deg cone (`7435`) with a clear line of sight. CanSeeChasee = visible or
  just told; KnowAboutChasee = 30 s memory; SuppressMoodAboutChasee / UnsetWantAndSuppressMoodForATime block mood
  events about the target; MoodResetAboutChasee forgets them; LostChasee walks to the last known position.
- Secondary chasers (`b19`): every non-primary chaser runs UpdateBlockPrediction each tick; it blocks when the
  chasee is within 20 m and touching (1 m) or running at it faster than 5 m/s inside 45 deg (record fields
  `8BAE` / `F3E7` / `670A` / `E48B`); BlockChasee walks to the block point at 2 m/s and stands there facing the
  chasee (recovering exhaustion); else PursueChasee runs to its formation point = the chasee plus the offset it
  joined at.
- Tazers (`b20`): DrawTazer (0.133 s draw), TazeWantTarget (speech 66, after 0.3 s one hit through the target's
  `vfn20`: the same knock-down as a takedown, `826A8420`), RegisterTazer (live tazer), EndTaze (speech 67); the
  stock graph tazes within 20 deg and 10 m (IsFacingWantTarget: `826AA1B8`, half the angle either side, flat
  distance) with a line of sight.

**Change.** skate-core `living_world::peds::perception` (`Perceptions`, `sees`, `Sight`); `ChaseGroup` formation
offsets and `chase::should_block`; brain ops for all of the above (`ChaseSteer` Intercept / Pursue / Block),
`MoodStore::forget`. skate-game: per-type sight from `tables.json`, the perception tick in `think_peds` (eye 1.6 m
above the feet, line of sight = our navmesh line), suppressed instigators post no mood, Pursue / Block / LostChasee
routes, the taze hit writes the takedown's `SkaterTakedownRequest`, RegisterTazer plays the tazer burst
(`PedTazerEvent`). Logs `PED_TAZE`; mod event `ped_taze`.

**Engine choices.** The eye point, the line-of-sight ray (retail: physics rays) and the chasee radius (`G+1648`, 0.0)
are ours; the tazer is drawn when timer 37 ends (retail: the draw clip) and the `tazr` hand prop is not attached;
CanTazeUnreachableTargets and CannotReachChaseTarget stay Pending (their per-type byte and nav-result writer are
open), so the unreachable-taze branch does not run.

**Verification.** skate-core `entries_follow_the_retail_list_rules`, `vision_has_touch_near_and_a_cone_with_line_of_sight`,
`secondary_chasers_block_a_chasee_running_at_them`, `a_tazer_is_drawn_then_hits_its_target_once` (+ the earlier chase
tests). Not play-tested.

**Open.** Greets and conversations between peds (presence about other peds is retail, `sub_82E3CA20`, 35 m; waits for
the greet ops, research b22), the taunt after a takedown, hand props.

## Peds greet each other (2026-10-09)

**Retail [code] (`.local/research/peds/b21-ped-greets-conversations.md`, `b22-ped-greet-ops.md`; main checked the
radius constants and the greet speech).** Every 0.5 s a ped posts presence about the other live peds within 35 m (at
most 30, `sub_82E3CA20`) and then about players within 35 m. The stock greet results fire on presence (gated on the
other ped's type, probability 0.5, cooldown 20 s) and raise `greet`. ChannelGreetWantTarget (`8269FAC0` /
`8269FC30`): greet timer 3.5 s, posts `greeted` into the greeted ped's mood store (which raises `returngreet` there),
speech 56 (63 for the return greet), unsets the want when the timer runs out (`deactivateWant`; the graph's
`unsetWant` attribute is never read). ApproachWantTarget walks to the target at `speed` (2.0); AlertToWantTarget
pushes the target on the look-at stack. Correction to our warn op: ChannelWarnWantTarget unsets its want only with
`deactivateWant` (the unreachable chase keeps it).

**Change.** `mood::presence` takes the other peds (35 m, 30); brain ops ChannelGreetWantTarget, ApproachWantTarget,
AlertToWantTarget; `ChaseRequest::Greeted` queued into the greeted ped's store on its next think (log `PED_GREET`);
ChannelWarnWantTarget gains `deactivate`.

**Engine choices.** (The temporary hold-back of the `startconversation` results that this section first had was removed the same day, once the retail mood gate and the conversation plugin were ported: see "Ped conversations".) The world query order for the 30-ped cap is ours (id order); the
look-at stack has no head-tracking consumer yet; `greeted` reaches the target one think later.

**Verification.** skate-core `presence_posts_other_peds_then_players`,
`a_greet_posts_greeted_once_and_speaks_until_the_timer_runs_out`. Muted DownTown live run (25 s): 13 greets, return
greets answered, no ped held, 68 mood lines.

**Open.** Conversations (SpawnConversationArea, the conversation plugin and tables), the presence filter's exact
meaning (`ped+2020`), the `ped+2480` speech tag.

## Ped conversations: the plugin runner and the conversation object (2026-10-09)

**Retail [code] (`.local/research/peds/b23-ped-plugins-conversations.md`, `b24-conversation-object.md`; main checked
the spawn constants, the setup data chain, the conversation vtable and its completion state).** A conversation is a
waypoint plugin: SpawnConversationArea (`826A6CD8`) spawns one 3 m ahead of the starter unless another is within
50 m, and adds the starter and the greeted ped. In the main graph, HasPlugin moves a member into the Plugin state,
whose op runs the plugin's own graph (`plugin/conversation.stategraph`) on the same brain. The conversation object
(vtable `0x8232B868`): 3 member slots, 3 waypoints on a 1.5 m circle 120 deg apart; a member locks the nearest free
waypoint, walks onto it (`template/moveontowaypoint.xml`: 2.0 m/s, then 1.0 m/s, 0.1 m), turns to it, signals in
position; when all are in position it starts (fewer than 2 members ends it), picks a candidate row
(`livingworld_conversations`, via the entity's conversation group and weighted categories) and one value from the
row's list; 5 turns of 3.0 s round-robin (state 2 to 7), then complete; members unlock and exit the plugin.

**Change.** skate-core `living_world::peds::conversation` (`Conversation`, `ConversationRow`,
`ConversationParams`); brain ops Plugin, ExitPlugin, SpawnConversationArea, PedestrianInConversation /
PedestrianIsInConversation, LockClosestWaypoint, HasWaypointLocked, DistanceFromWaypointXZ, TargetWaypoint,
LockToCurrentPosition, TurnToFaceWaypointOrientation, IsFacingWaypointOrientation, UnlockWaypoint,
CreateSimpleMonitoredIntent, IncrementMonitoredPacketStage, HasMonitoredIntent (any intent), ConversationSignalInPosition,
ConversationThisParticipantIsSpeaker, ConversationSpeak, ConversationListenToSpeaker, ConversationIsComplete.
skate-game: the conversation plugin graph and tables load with the ped data, `PedConversations` (host-owned, seeded
RNG), the plugin graph runs on the brain while the main graph is in Plugin, requests applied in `think_peds` (log
`PED_CONVERSATION`).

**Mood gate (`b28`, `b29`, main checked `sub_82E3BF70` and the producer's `ori 32`).** The mood producer
(`sub_82E41B98`) does nothing for a busy ped (`sub_82E3BF70`: WaitingToReact `brain+3278` 0x20, IsReactingToMoodEvent
0x10, `brain+3277` 0x04 (written by PedestrianColliding), `*(ped+96)`); a pass sets WaitingToReact itself; the want
groups in `pedestrian_addresswants.xml` clear it when they begin (and set / unset IsReactingToMoodEvent). Check 8 of
a result (`D44E`) compares the target's busy state. A post whose reaction-set prerequisites fail is dropped
(`sub_82E41060`; presence: within 12 m in the stock sets). Ported: the four flag ops, `PedBrain::busy` (ours: the two
open terms are a plugin membership), the gate in `think_peds`, check 8 (`MoodContext::busy`), prerequisites at post
time for presence (`MoodTables::accepts`). By the code (`b29`) a ped whose conversation spawn is refused (another
within 50 m) raises the want again on its next pass, as in retail; we keep that (no heuristic stop).

**Engine choices.** Waypoint orientation = towards the centre; TargetWaypoint's slide moves the body straight onto
the point (no slide clip); LockToCurrentPosition pins the body to where it stood (an origin-locked trajectory: root
motion does not move it) and drops the route; ConversationListenToSpeaker only records the speaker to look at (no
body turn: the body keeps the waypoint orientation; no head tracking consumer yet); the waypoints have no navmesh probe; players are never busy for check 8
(unverified).

**Live run (muted DownTown, 45 s).** Three conversations spawned; one ran its 5 turns with alternating speakers and
completed (logs `PED_CONVERSATION ... turn state=3..7`, `PED_PLUGIN`). Open from that run: two conversations whose
members never all arrived, a facing jitter between TurnToFaceWaypointOrientation and the converse states (the 0.3
rad check at the threshold), and about 30 mood lines a second from peds whose spawn was refused (retail-identical
loop by the code).

**Verification.** skate-core `a_conversation_gathers_starts_and_runs_five_turns`,
`a_lone_member_ends_the_conversation` (and the tests of the next section); skate-game data-gated `living_world_ped_plugin_graphs_load` (conversation, sit,
lookat, spectate graphs load on the runtime).

**Open.** The facing jitter (above), prerequisite kinds 2 and 8 (OwnPluginObject / DisableCollisionsWithBehaviourSource flag,
`brain+540`), the other plugins (sit, ATM, vending, ...).

## Conversation speech, gather timer and abort (2026-10-09)

**Retail [code] (`.local/research/peds/b43-conversation-speech-slots.md`; main re-read `82E3DAD8`).**
- Start (`82E1EBC0`) and every advance (`82E1ECB0`) store the turn's line id (`82E1EA98`: state 2 -> 0 or 1, 3 -> 3,
  4 -> 5, 5 -> 6, 6 -> 7, at `D+1168`) and re-roll the variant (`D+1156`, uniform [0, 2) by `82E17478`) once per turn.
- ConversationSpeak Begin (`826A66B8`) calls `82E3DAD8`: the speaker's speech value (`ped+2468`, the channel of every
  ped speech) becomes line id + 33 (33 / 34 intro short / question, 36 opinion, 38 question, 39 answer, 40 outro),
  with the variant at `ped+2472` and the row's value at `ped+2476`; every listener (vf32 `82E1E800`: members other
  than the speaker) gets 41, which no audio event uses. Because PedestrianSpeech speaks on a change, the 41 makes the
  next line of the same value audible.
- The first join (`82E1D380`) sets state 1 and a 30.0 s gather timer (`D+1136`, `0x820D4924`); `82E1EB10` counts it
  down and starts the conversation when it runs out, even if not every member signalled in position (fewer than 2
  members then ends it, vf52 `82E1E998`: state 7, no speaker).
- A member leaving (`82E1D470`) aborts an unfinished conversation (vf52); the last one leaving resets it, and a
  spawned area (destroy-when-empty, set by SpawnConversationArea) is removed (`82E1BD08`).

**Change.** skate-core `conversation.rs`: `line` / `variant` per turn, `TurnSpeech` (`turn_speech()`), `listeners()`,
`tick(dt)` (gather timer), `leave` returns `Left` (NotMember / Remaining / Empty) and aborts, `pass_turn` takes the
seeded rand; `ConversationParams.gather_seconds`. Brain: ConversationSpeak Begin writes the speaker's value and
`PedBrain.speech_topic` (variant, row value) and posts `ChaseRequest::Spoke`; the host gives the listeners 41 when
they think next. `PedSpeechEvent.topic` and the mod event `ped_speech` (`variant`, `list_value`) carry the topic.
Logs: `PED_CONVERSATION ... gather_timeout`, `leave ... result=abort|empty|leave`, `PED_SPEECH ... topic=`. Mod values
`ped_brain.conversation_turn_seconds` (3.0) and `ped_brain.conversation_gather_seconds` (30.0), 0..=100.

**Engine choices.** Every conversation area is spawned (no placed map areas yet), so an empty one is removed; the
leaver's slot key is not kept (retail keeps it for a re-join by the same entity type); a listener whose think already
ran this tick gets its 41 on the next tick.

**Verification.** skate-core `the_line_and_variant_are_rolled_once_per_turn`,
`the_gather_timer_starts_with_whoever_is_there`, `a_member_leaving_aborts_and_the_last_one_empties_it`, and the five
turns test now checks the values 33, 36, 38, 39, 40 and the listeners; skate-game `living_world` tests pass.

**Open.** What the variant and row value select on the audio side (readers of `ped+2472` / `+2476` not found); the
waypoint vfunc 24 body.

## Peds taunt after a takedown (2026-10-09)

**Retail [code + data] (`.local/research/peds/b25-ped-taunt-handprops.md` sections 1-2; main checked the 65 / 19 speech
values).** After a successful takedown (or an aggressive capture) the graph turns the angry-chase want into a taunt
want; `TakedownTauntVictim` (`826A70E0`) speaks 65 when the victim is the player and 19 otherwise, faces the victim,
pushes attention and posts the "Taunt" motion intent: `motiongraph_taunt` plays the ped type's remapped "Taunt" clip
once (blend 0.1) and completes the intent; the graph holds `DoTaunt` while the intent lives. End (`826A72F0`) releases
the face and the intent and unsets the taunt want.

**Change.** skate-core brain op `TakedownTauntVictim` (Begin: face the target, monitored "SGIntent", `ChaseRequest::Taunt`;
End: face off, intent off, want unset); "Taunt" in the logical animation names. skate-game `think_peds` speaks 65 / 19
and requests the clip (`PedBody.taunt`), the body step plays it through the reaction path and the intent is dropped
when it ends (a set without the clip completes at once). Log `PED_TAUNT`.

**Engine choices.** The attention push (`sub_82E27A08`, priority 2) is not modelled; the clip plays through the collision
reaction player (no locomotion while it runs), as the motion graph's state is non-interruptable.

**Verification.** skate-core `the_taunt_faces_the_victim_holds_the_intent_and_unsets_the_want`; skate-game living_world
(71). Not seen in game yet. Open: the hand props (throw / drop), which our peds do not carry.

## Ped plugins on world props: data, rolls and the offer scan (groundwork, 2026-10-10)

**Retail [code + data] (`.local/research/peds/b81-ped-plugin-acquisition.md`, `b82-ped-plugin-search.md`,
`b83-plugin-offer-and-radii.md`; main checked the ped roll `82E3B060`, the d100 roll `8269A588`, the offer test
`826BFE18` and its constants, the sit ops and the stock tables).**
- Plugin props: benches, trash bins, newspaper boxes, vending machines, ATMs, water fountains, look-at and spectate
  spots, gathering places. Each prop class (`livingworld_props`) names a descriptor (`PluginDescriptor/*.xml`: the plugin
  graph, `maxNumberOfParticipants` default 1, a transfer expression of named conditions) and hand-prop odds. Placed
  waypoint groups come from the world (`waypoints.json`: vending machines, trash bins); seats come from DMO hotpoints
  (`82C4E128`: type 1 trash bin, 2 newspaper box, other = a seat, one waypoint each).
- A ped type's `plugin_odds` (per class) drives the ped's yes / no when offered (`82E3B060`: odds x 100 >=
  `rand() % 100 + 1`) and the spawner's weighted pick of a ped type to spawn at a free prop (`826B90F8`).
- The offer scan (`826C0058`, every 11th manager tick at 1/60 s) walks every ped for every offer: no current plugin,
  the prop not refused (`82E3AF90`), |dy| < 1.8 m, within the offer radius, the offer's own test, then the odds roll;
  the winner takes the waypoint. A refusal (and a taken plugin, `8269F248`) blocks that prop for that ped for 30 s
  (`82E40BD0` / `82E40AF0`: at most 16 entries, the countdown stops at -30).
- Sit (`sit_body.xml`): SetSitTimer (`826A2898`) = 30 + rand x (60 - 30) s for the stock types, then GoingToStandBackUp
  (`826AD1F0`) rolls the type's stand-up chance (0.5); a ped that loses the roll stays seated.

**Change.** skate-core `peds::plugins` (descriptor and transfer-expression tree, plugin props with waypoint locks and
cooldown, `plugin_odds` lookup, the d100 roll, the refusal memory, the ped roll, the offer scan, the spawn pick, the
hotpoint class map) and the sit ops in `peds::brain` (`SetSitTimer`, `GoingToStandBackUp`, per-ped seeded rolls,
`SitValues` from the tables). skate-game `ped_plugins` loads the prop classes with their descriptors (from the compiled
`.stategraph`), the `plugin_odds` per ped type and the placed props per district; `PED_PLUGINS` log.

**NOT RETAIL YET / not wired.** Peds do not take plugins yet: the plugin graphs other than the conversation, 25 of
their operations, the hotpoint export (benches) and the monitored-intent animations are still to port. Open in the
research: the offer radius (ours will read the class field `E2101F2B17A0E6B5`: 10 sit / 5 bin / 15 ATM, inference), the
offer's own tests (vfuncs 9 / 10 / 18), the spawn ring radii and the cooldown source.

**Verification.** skate-core `plugins` (6: hotpoint classes, waypoint locks and cooldown, the ped roll and the refusal
memory with eviction and floor, the offer scan's filters, the weighted spawn pick, transfer expressions) and the brain
sit test; skate-game `ped_plugins` (parsing) and the data-gated `stock_descriptors_parse` (sit's transfer conditions,
the conversation's 3 participants, adult_female01's sit odds 0.7, placed props DownTown 27 / Industrial 10 /
University 37).

## Ped plugins on world props: motion states and the wiring (2026-10-10)

**Retail [code + data] (`.local/research/peds/b84-plugin-intent-animation.md`, `b85-packet-completion-and-clips.md`;
main checked the packet ops `826A2600` / `826A27D0` / `826ACF90` / `826A2770`, `LockFirstWaypoint` `826A0580` and the
motion XML).** The plugin body graph creates a monitored packet (one intent per stage, e.g. Sit then StandUp); the
AI side only moves its stage; the ped motion graph plays a state per packet (Sit: Stand2Sit, SitIdleCyc held until
StandUp, Sit2Stand; ATM: insert card, select, collect money, collect card; vending; water fountain until
FinishDrinking; newspaper box) and ends the packet (`IntentStageComplete decrement` on the machines,
`MajorIntentComplete` clears the active flag). The clip behind each logical name is the ped's anim set remap (sit per
set: NPC / FEM / BUM / GRAN; the machines shared). `LockFirstWaypoint` locks the prop's first free waypoint.

**Change.** skate-core: monitored packets as retail (stage names, active flag, the Create End removes the packet),
`plugin_motion` (the five plugin states as step tables), `PedAnimPlayer::release_hold`, the plugin clip names in the
loader list, `LockFirstWaypoint`. skate-game: each class's plugin graph loads from its descriptor; `PedPluginProps`
holds the map's placed props (waypoints.json), runs `plugins::offer` every 11th world tick with the class radius and the
ped type's `plugin_odds` (seeded), releases waypoints of peds that left; a ped that took a prop has HasPlugin, its
plugin view is the prop's waypoint (facing point along its orientation) and its plugin graph is the class's; ExitPlugin
releases the waypoint. The motion side plays the front active packet (`PED_PLUGIN_MOTION`) and ends it; Converse keeps
the last-stage fallback. Logs `PED_PLUGINS`, `PED_GRAPH plugin <class>`, `PED_PLUGIN_PROP`.

**NOT RETAIL YET / open.** Benches (DMO hotpoint seats) are not exported yet, so only the placed vending machines and
trash bins are offered; the offer's own tests (vfuncs 9 / 10 / 18) and the radius source are open (radius = the class
field `E2101F2B17A0E6B5`, inference); the step tables stand in for the motion graph; hand props (vending can,
newspaper, trash throw), the sit-avoid states and the Collision exits are not ported; the packet queue is one packet
per name (retail queues them).

Two body rules came out of the run: within TargetWaypoint's slide distance the body stands while the slide moves it
(it used to wander off before reaching 0.025 m), and a ped standing on its waypoint keeps turning to the explicit turn
direction that SetExplicitTurnDirectionToWaypointOrientation and TurnToFaceWaypointOrientation set (`826A0DC8`); without it
the facing test flickered at its 0.3 rad edge and restarted the interaction. Logs `PED_PLUGIN` (with the distance to the
waypoint and the position) and `PED_PLUGIN_WAIT` (every 2.5 s while on a prop).

**Verification.** skate-core peds (packets, the sit sequence's hold and release, plugin motion names), plugins (6),
living_world 255; skate-game living world / ped / npc / modding 159; data-gated `placed_plugin_waypoints_against_the_navmesh`
(every placed waypoint on the mesh or unlocated). Muted Industrial run with the release build: ped #2 takes the vending
machine `CD3BB21DBDF4C550` at tick 10, walks 10 m, slides to 0.019 m, faces it, plays the vending clips (8 s), says its line,
leaves (tick 1098) and is offered it again once its 30 s memory runs out. Not play-tested. Open: the conversation's
Speak / TurnToFace flicker (conversation waypoints have no orientation).

## Ped plugins on world props: benches, bins and newspaper boxes from the model hotpoints (2026-10-10)

**Retail [code + data] (`.local/research/peds/b86-dmo-hotpoints.md`, main checked the parse on the user's disc).** A dynamic
object's seats, bin and newspaper-box points are hotpoints in its model RX2 in `parkassets.big` (RW type 0x00EB001E
HOTPOINTDATA: count, entries at +16, 112-byte records with a row-vector matrix whose row 3 is the position and row 2 the
facing axis, +96 the type). `82C4E128` makes one one-waypoint group per hotpoint when the object spawns: type 0 is the root,
8 is skipped, 1 a trash bin, 2 a newspaper box, any other type a seat. On the disc: 22 templates, 30 seats, 7 newspaper-box
points, 5 bin points (e.g. a flat bench has 4 seats, a picnic table 4, a patio chair 1).

**Change.** Setup: `tools/asset_pipeline/hotpoint_data.py` (strict parser, the class map as data) reads parkassets.big in
`prepare_catalog` (`hotpoints.json`); the map's props report gains `hotpoints`, `hotpoint_classes` and `plugin_props` (each
placed object's hotpoints with a class, through the same transform as its geometry). Game: `ped_plugins::map_props` adds
them to the map's plugin props (id = DMO instance id with the hotpoint index in the top byte); `PED_PLUGINS <map>: N
hotpoint props` log. Brain: `DisableCollisionsWithBehaviourSource` (`826A7378` / `826A7410`).

**NOT RETAIL YET / open.** Needs the `maps` setup group re-run (the current install has no `plugin_props` yet, so benches
are not offered until then). Seats follow the initial placement (a moved bench keeps its seats where it stood); the ped is
not yet exempt from its plugin prop's navmesh cut (the DMO instance to prop id mapping is open), which a seat inside the
bench's cut needs; the EMBEDDEDDATA list that links a template to its hotpoint records is not decoded (one section per
RX2 makes the template join safe); 13 of the 22 templates are not placed through worlddmo.

**Verification.** Python `test_hotpoint_data` (parse, header / matrix checks, the placement transform and the class map),
`test_dynamic_props`; skate-game `hotpoint_props_load_with_stable_ids`, living world / ped 103. The disc parse: 22
templates, types {6: 30, 2: 7, 1: 5, 0: 1}. Not play-tested.


**Live check (2026-10-10).** Muted Industrial run on the refreshed install: ped #2 took a bench seat after the vending
machine, reached the seat waypoint (0.048 m off), turned to its orientation and sat (`Sit` motion, `SitForever`). Our
prop-obstacle stand-in logged `PED_BLOCKED` against the bench while the ped slid along its face, but it arrived, so the
navmesh-cut exemption is not needed for this seat. The seated ped was later removed by the retail distance cull: the idle
player rolled away from the spawn into the bowl at 11 m/s (fast census circle with its forward offset); `PED_DESPAWN` now
logs each removal with its reason.

## Ped hand props: the vending machine can, the newspaper (2026-10-10)

**Retail [code + data] (`.local/research/peds/b87-ped-hand-props.md`, `b89-hand-prop-models.md`, main checked the key
addresses).** The vending machine and newspaper box motion states run `SpawnInteractionBasedHandProp` (Begin `826AE950`) in the
collect clip's branch window (`InTurnBranchWindow`). It asks the ped's plugin object (vfunc +52) for a hand prop key and
`82E3DDA0` stores the `livingworld_handprops` record and sets the requested bit (`brain+3279` 0x01, which `HasHandProp`
`826AD1A8` already counts). The ped update later creates the object (`82E3DE18`: the record's `model` is a `dmo_models`
record whose first field is a DMO template id, looked up in collection `0xDA7A8F0EF63BE539`) and attaches it (`82E3EC60`:
`ped+5920`, `brain+3278` 0x02). Every frame the object is put at the hand matrix composed with the record's local offset.
`IsHoldingSpecificHandProp` (`826AD270`) compares the held record's key with its `handprop` attribute. Which props a plugin
offers is data: `livingworld_props` field `E15E856F2CA9B96B` (vending machine: pop 0.5, waterbottle 0.5; newspaper box:
newspaper 1.0). The `_lw` models are DMO templates in `worlddmo.big` `DMO_Global` (one mesh each); 22 of the 23 records
resolve (`orange_lw` is in no DMO pack).

**Change.** Setup: `tools/asset_pipeline/hand_props.py` writes `living_world/hand_props/<key>.glb` from the DMO catalog
template (model space, texture) plus `hand_props.json` (key, model, template, characteristics, or the error); `dmo_models`
joins the exported vault classes in `tables.json`; it runs in the `maps` group after the DMO catalog. Core: `PedBrain.hand_prop`
(`HandProp`: key, requested, holding; the weighted pick), conditions `HasHandProp` and `IsHoldingSpecificHandProp`, and the
`hand_prop` flag on the collect steps of `plugin_motion`. Game: the motion run flags the collect clip's branch window,
`think_peds` picks the key with the ped's seeded brain RNG (`PED_HAND_PROP ... requested`, mod event `ped_hand_prop`),
`ped_hand_props::sync_hand_props` creates the object as a child of the ped's scene (`... created`) and removes it when the
brain drops the prop, and `present_ped_pose` places it at the rig bone `RIGHTHANDPROP` (26, 7.5 cm off `RIGHTHAND`) times the
record's offset. A one-entry hand prop list (the newspaper box) is exported as a bare record; the loader now takes it.

**NOT RETAIL YET / open.** The hand bone is inferred (every carry channel is `*RH`; which bone fills the hand matrix at
skeleton +19504 is not decoded). The record offset mapping (`Hash_3FE1...` = Euler degrees, `Hash_DC20...` = metres,
applied as translation then X, Y, Z) is inferred from the value shapes. The weighted pick by the plugin object is inferred.
The object is drawn only: no physics body, no carry channel, no release (`ThrowHandPropAtTrashBin`, `DropHandProp`,
`HasDisposableHandProp`, `CanSitWithHandProp` are not ported), and what removes a released prop is open. Starting props on
walking peds (the spawn path `82E33198`) are not ported.

**Verification.** skate-core `hand_prop_conditions_and_the_weighted_pick`, `every_plugin_state_is_listed_and_its_names_load`;
skate-game `record_offset_is_translation_then_rotation`, data-gated `stock_hand_props_load_with_models` (22 of 23 with
models), `stock_descriptors_parse` (hand prop lists incl. the bare record), `living_world_ped_data_loads_from_the_export`
(`RIGHTHANDPROP` = 26 under `RIGHTHAND`). Offline export on the user's disc: 22 GLBs (pop can 7 x 12.8 cm, rendered).
Muted Industrial run: the ped at the vending machine requested and created `pop` in `VendCollect` (tick 1051) and carried it
to a bench. Not seen on screen yet.

## Ped hand props: the bin throw, the drop and the released prop (2026-10-10)

**Problem.** Peds held their vending can or newspaper forever: the release ops (`ThrowHandPropAtTrashBin`,
`DropHandProp`), the hand prop conditions (`HasDisposableHandProp`, `CanSitWithHandProp`, `CanAttackThrowHandProp`) and the
world prop descriptors' transfer gates were not ported, so `usetrashbin.xml` exited at once and any ped took a bin.

**Evidence** [code] (TU3 recomp, reference only; research b25, b87, b90, b91, b92; main-checked):
- `sub_82E3E648` starts a throw only while holding: pending + aimed bits, target, launch speed and release time; the light
  throw (bin) is 5.0 m/s (`0x821F1790`) after 0.91667 s (`0x82063BE0`), the attack throw 10.0 / 0.8333. Its clip by the
  flat angle to the target: below 0.6854 or above 5.5978 rad `HandPropThrowLightForward`, then `L45` (< 0.8854), `L90`
  (< 1.6708), `R180` (< 4.6124), `R90` (< 4.8124), else `R45` (thresholds `0x822F9144..58`, names at `0x82064A00..7C`).
- `ThrowHandPropAtTrashBin` (`826A7998`) aims at the plugin object's hotpoint 0. Every stock bin has one hotpoint.
- The launch solve `sub_82E16AB0`: the flat speed is the launch speed, flight time = flat distance / speed, vertical speed =
  dy / t + 4.9 t (`0x822F8FA4`); no speed or a target straight above gives zero velocity and time -1 (`0x8216DEE0`).
- The per-ped update `82E3ED50` releases when the release time has run out, with the launch velocity from the prop's hand
  position, and sets timer 35 to the flight time; `82E3EBE0` clears holding and calls the object's release (+136,
  `82C56C70`: kinematic off, velocity written only for flag 1; DropHandProp passes flag 0). A released prop stays linked
  until it leaves the box 0.5 / 2.0 / 0.5 m around the ped (`82E3F090` -> unlink `82E3FAE0`); the ped never destroys it.
- The dynamic-object manager's create (`826B8830`) refuses at 49 live objects [code, b91]; the census culls
  `dynamicobjects` beyond 100 m (`livingworld_census_ranges` [data]). The record bools: IsDisposable (+60, `Hash_D02B...`),
  CanSitWithHandProp (+61, `Hash_BCE5...`), CanAttackThrowHandProp (+62, `Hash_4080...`) [b90].
- A thrown prop hitting the skater has no special effect: the DMO class posts nothing and the skater contact switch treats
  groups 12 / 13 / 14 as an ordinary prop contact [code, b92].

**Change.** Core `peds/hand_prop.rs`: `HandPropSettings` (retail defaults, in `BrainSettings.hand_prop`; mod keys in the
`ped_brain` domain since the zombie step: `attack_throw_speed`, `attack_throw_lead_seconds`, `attack_throw_jitter`,
`attack_throw_lift`, `light_throw_speed`, `hand_prop_skater_contact`, `starting_hand_props`), `launch`, `flat_angle`, `light_throw_clip`, `PedBrain::start_light_throw` /
`drop_hand_prop` / `update_hand_prop_release` / `update_hand_prop_link`. Brain: `HandProp` gets `linked`, `throw` and the
record bools; ops `ThrowHandPropAtTrashBin`, `DropHandProp`, `DisallowHandPropActions` (flag); conditions
`HasDisposableHandProp`, `CanSitWithHandProp`, `CanAttackThrowHandProp`; requests `HandPropClip` / `HandPropReleased`;
`plugin_target` (host-set hotpoint 0). Plugins: `TransferView` evaluates the world prop descriptors' transfer
conditions in the offer scan. Game: the request carries the record bools; the throw clip plays on the ped;
`sync_hand_props` clears the request bit on the create attempt, releases the held object as a prop created mid-game
(doc 27 "Props created mid-game": collision from the model's meshes, its model detached from the hand and following the
body), keeps the link until the unlink box, refuses creates past 49 live released props and culls released props beyond
100 m of every observer. The release test and the body's start pose use the tick's hand pose, not the drawn one
(doc 27 "Census camera and hand prop release from the tick"). Logs: `PED_HAND_PROP ... throw clip`, `... released body=.. velocity=..`, `... unlinked`,
`... culled`, `... not created (pool full | no model)`.

**NOT RETAIL YET / open.** The release frame: retail compares a clip time query (skeleton +17936 vfunc +36) with the
release time; ours runs timer 34 out (equal if the query is the time remaining, inferred). The drop keeps the hand's
velocity in retail; ours is zero. The released body uses the default physics block and box: the hand prop template's
`livingworld_dynamicobject_characteristics` record is not resolved (b91), collision groups 12 / 13 / 14 are not modelled.
The pool and cull are counted over released hand props only (retail's pool holds every live DMO). The flat angle's sign is
inferred from the clip names. `IsNearCrossWalk` answers false. The attack throw: see the next section.

**Verification.** skate-core `launch_reaches_the_target`, `light_throw_clip_by_angle`, `bin_throw_releases_then_unlinks`,
`drop_releases_with_zero_velocity`, `world_prop_transfer_reads_the_hand_prop`; skate-game data-gated
`stock_hand_props_load_with_models` (pop disposable, newspaper not). Muted Industrial run (6 min, after fixing the
vending / sit gates `HasFirstWaypointAvailable` / `InFrontOfFirstWaypoint`, which the first run showed): ped #2 took the
vending machine, created `pop` at tick 1051 and sat on a bench with it (pop allows sitting); no ped took a bin in the run,
so the throw is not seen in game yet.

### Zombie mode: the ped side

**Problem.** The zombie cheat (free skate: peds chase and attack the skater) had only its census rules; the brain's
`IsZombieMode` read a flag nobody set, and the stock graph's `ZombieFollow` / `OverrideAnimData` ops were not ported.

**Evidence** (research b95; main-checked: the ZombieFollow update `826A9250` distances and speeds, constants
`0x82099250` 8.0, `0x820BD16C` 15.0, `0x8231A844` 1.0, `0x82063B08` 3.0):
- Stock graph [data]: `Wander` -> `ZombieFollow` when `IsZombieMode`; the state runs `OverrideAnimData overrideEntityName="zombie"`,
  `DropHandProp`, `ZombieFollow`, and leaves to Colliding / AddressWants, or to Wander when the mode is off. Scatter, the
  chase start timer, the primary-only intercept, rest and the taunt are all gated off in zombie mode by the stock graphs.
- ZombieFollow [code]: Begin `826A91A8` motion intent 0, goal = the player. Update: beyond 8 m the goal is the player;
  inside, once the goal is reached (steering vfunc +124, inferred), a random point 1..8 m round the player
  (`82E17508`, the same jitter as the attack throw); speed 8 m/s beyond 15 m, else 3 m/s. End `826A9518` resets.
- `SendSpeechEvent` Begin `826A77A8` sends nothing in zombie mode [code, b95].

**Change.** Core: ops `ZombieFollow` and `OverrideAnimData { entity }` (cleared on End, inferred),
`IgnoreTakedownTargetNavRigVolume` as a marker; `ZombieFollowSettings` (retail defaults); brain `zombie_goal`,
`anim_override`; no ped speech in zombie mode. Game: the brain's `zombie` flag follows `LivingWorldSettings.zombie`
each tick; the body walks a one-point route to the zombie goal at the brain's speed; the anim set override picks the
`zombie` set. Toggle (no cheat screen yet): `SKATE_ZOMBIE=1` or the mod value `living_world.zombie`; mod keys
`ped_brain.zombie_follow_distance`, `zombie_sprint_distance`, `zombie_ring_min`, `zombie_ring_max`,
`zombie_sprint_speed`, `zombie_walk_speed`, `zombie_arrive_distance` (goal reached, 2.0 m 3D: the mover arrival radius, b158 / main-read `82E2BC70`, `82E35E80`).

**The yellow screen (research b100 / b101; values main-checked in the stock collections).** `827F4D28` sets post FX
bit 0x2000 while the zombie query is true and `827F5450` then builds the near and far colour matrices from the `post_fx`
record `colour_matrix_zombiemode` (`827F13D8`: `multiply * Sat_s(contrast * (x - midpoint) + midpoint + add)`, luma
0.3 / 0.6 / 0.1) instead of the identity `colour_matrix`, a full replace with no fade [code]. Values [data]: multiply
(2.0, 1.4, 1.0), saturation 0.5, contrast 1.3 near / 1.5 far, add 0 near / -0.4 far, midpoint 0.5, distances 50 / 100.
`postfx_visualfxPS` applies them after the tone curve: near rows c7..c9, far rows c10..c12, mixed linearly in view
depth (`827F3D20`: full near to 50 m, full far from 100 m) [code / data]. Ours: skate-core `colour_matrix` (builder),
skate-game `colour_matrix` (stock records, `ColourGrade` picked by the zombie flag, the camera's near plane), the retail
tone pass reads the view depth (gameplay camera depth now sampleable; a 1x1 fallback when it is not) and applies the
grade before its clamp. The base record is the identity, so the picture is unchanged outside zombie mode (DownTown
captures with the mode off and on). Checked 2026-10-29 (research b679; uploads by FNV-1 32 name hash in `827EC1C8`):
in zombie mode the stencil mask term is off (`f_StencilLerpMod` c16.x = 0.0 from the zombie branch of `827F5450`; 1.0
otherwise), so the grade covers every pixel as ours does; the tint LUT never applies (`f_bTint.x` = 0: `827F4D28`
clears mask bit 0x10 every frame); the bloom-colour desaturate step's weight is a timed fade value (`827EF1F0`,
`82795A70`) zombie mode does not touch, at rest presumably 0 (identity). Nothing to port for zombie mode; plain play
draws no visualFX pass at all (mask 0x05). Open: that fade's resting value and driver, which draws set stencil 0x80.

**NOT RETAIL YET / open.** The cheat screen entry belongs to the UI work. The "goal
reached" radius (0.5 m) and "ZombieChannel" (`ped.vfn240`, 0.2 / 0.2 / 1.0) are not read; how peds in a plugin become
zombies is open. Retail aims every zombie at the single local player; a host picking per ped is not retail.

**Traffic.** Retail's vehicle cull `826BAAB8` despawns a car beyond the cull radius OR whenever the zombie query is true
[code, main-read], so turning the mode on empties the streets; ours: `CensusKindConfig::cull_in_zombie` (vehicles true,
peds false), reason `Disabled`; test `zombie_mode_culls_the_live_traffic`.

**Verification.** skate-core `zombies_follow_the_player_and_mill_round_them`; skate-game data-gated
`living_world_zombie_mode_runs_zombie_follow_on_the_stock_graph` (stock graph: ZombieFollow, intent 0, goal the player,
8 m/s, `zombie` set, prop dropped; back to wander when off); skate-mods patch validation. Not seen in game yet.

### Peds that start out carrying a hand prop

**Problem.** Only peds that used a vending machine or a newspaper box ever held a prop; retail also gives walking peds a
starting prop (grocery bags, purses, briefcases, coffee, skateboards, maps, ...), and the angry throw needs one.

**Evidence** [code, main-read]: the ped constructor `82E33198` rolls `8269A588` with the entity's field
`Hash_3DB019A08284F45C` (chance; `rand() % 100 + 1 <= chance x 100`), then walks its `handprop_odds` list (field
`Hash_46B836EE959C0238`, at most 32 entries of {handprop, probability}): a second `rand() % 100 + 1` roll picks the first
entry whose running total x 100 (`0x820ED57C` = 100.0) reaches it, and `82E3DDA0` stores the record and sets the request
bit (no pick: the bit is cleared). The weights are not normalised. Stock [data]: 0.65 for adult women and grannies,
0.6 for adult / business / worker men and skaters, 0.5 jocks and teens, 0.75 bums, 0.1 security (coffee), 0 for pros.

**Change.** Core `HandProp::starting_pick(chance, list, chance_roll, pick_roll)`; setting `hand_prop.starting_props`
(mod opt-out). Game: `ped_mood::starting_hand_props` loads the chance and list per entity type into
`PedData.starting_props`; each ped rolls once with its seeded brain RNG when its brain is first set up, and the request
goes through the existing create path (`PED_HAND_PROP ... requested <key> at spawn`).

**NOT RETAIL YET / open.** The constructor also needs bit 0x80 of byte +112 of the spawn description before rolling; who
sets it is not found (all our ambient peds roll). Retail's rolls come from its global generator. The carry channels
(`CarrySmallRHChannel` etc., record +56 applied in pose state 7) are not played yet, so the arm keeps the walk pose.

**Verification.** skate-core `the_starting_hand_prop_follows_the_chance_and_the_running_total`; skate-game
`starting_hand_props_load_the_chance_and_the_list`. Not seen in game yet.

### Ped carry poses and the ped channel

**Problem.** A ped holding a can, a bag or a newspaper kept its walk pose: the arm swung through the prop.

**Evidence** (research b99; main-checked: the clip channel weights from the bank, the logical names in the anim set):
- Retail plays the carry pose on the ped's one channel slot ("PedChannel", the skeleton controller at
  `skeleton+17936`) [code]: ped vfunc +240 (`82E3A418`) looks the logical name up in the ped's anim set and requests it
  (`82E32878`: blend in / out); the per-frame state machine `82E3A5A0` plays it, fades it and frees the slot (state 7).
  While a prop is linked and held, the update `82E3F048` re-requests the record's carry name whenever the slot is free
  (0.2 s blends); the unlink `82E3FAE0` stops it with a 0.2 s fade. The same slot plays the throws, the tazer and the
  greet / warn gestures; a request replaces what plays.
- Which bones a channel clip takes over is in the clip data [data, main-read]: the clip parts' channel weight tables.
  All four carry clips (`NPC_CARRY_{SML,BIG,PAPER,WINE}_RH_0_CYC`, logical names `CarrySmallRHChannel` /
  `CarryBigRHChannel` / `CarryPaperRHChannel` / `CarryWineRHChannel`) weigh exactly the right arm chain (shoulder, arm,
  forearm, hand, hand prop bone) 1.0 and every other bone 0; the throw and tazer clips weigh 17 upper-body bones; walk
  cycles have no table. The blend is retail's `ACSChannelBlend` (already ported, `pose_blend::channel_blend_sample`):
  per bone, coefficient = channel fade x the bone's weight.

**Change.** skate-data: `PedClip.channel_weights` from the parts' weight tables. Core `peds/anim.rs`: the channel slot
(`channel_request` by logical name, `channel_stop`, `channel_idle`; fades during `step`; one-shot clips fade out at
their end) blended over locomotion in `pose`; the carry names join the clip list. Game: while a hand prop is held and
the slot is free, its record's carry name is requested (0.2 / 0.2); a released prop stops a carry pose with a 0.2 s fade;
the throw clips now play on the channel (upper body over the walk), as retail.

**NOT RETAIL YET / open.** Retail requests the carry while the prop is linked; ours while held. The tazer, greet and
warn gestures do not use the channel yet; `StopChannel` (Avoid / Collision states) is not ported. Not seen in game yet.

**Verification.** skate-data data-gated `carry_clips_carry_per_bone_channel_weights` (the four carry clips: 5 of 50
bones, the right arm chain; throws / tazer 17); skate-core `the_channel_overrides_only_its_weighted_bones_and_fades`.

### A ped on the phone drops it for a chase (2026-10-27, ask 391)

**Problem.** A ped on the phone that started an angry chase kept the talk pose for the whole chase (session
game-20261027-101012, ped #189). The user: "got chased by a ped on a phone, but he kept being on the phone while
chasing which looks weird and incorrect."

**Root cause** (research b649, `.local/research/b649-phone-ped-chase.md`; main checked the code lines). Retail
[data] `pedestrian_newchasee.xml` `HoldBeforeChaseStart` drops a prop that cannot be attack-thrown (`CantThrowHandProp`
-> `DropHandProp`, no hang-up clip); [code] the idle action update `82E3F470` runs only in the held branch of
`82E3ED50`, so the talk is never asked for after the drop; the unlink `82E3FAE0` stops whatever clip the channel plays
with a 0.2 s fade (`DAT_82099280`). Ours dropped the phone as retail but kept the stored talk cue and asked for it again
every time the channel idled, and the unlink stopped only `NPC_CARRY` clips (every frame, not once).

**Change.** `peds.rs` `cue_clip`: an action cue plays only while the prop is held; `ped_hand_props.rs` clears the cue
on the release and the removal. The unlink (box test, prop gone, release without a body, prop removed) counts
`PedMind::unlink_seq`; `advance_peds` stops the channel once per new count (`unlink_due`) with
`HandPropSettings::unlink_fade` (retail 0.2, a mod setting next to `unlink_box`); the per-frame `NPC_CARRY` stop is
gone. Ours, not read in retail: the object flee skips the ped's own linked hand prop (the ped fled its own falling
phone, `PED_FLEE_OBJECT prop=536870912` tick 40124, cutting `HoldBeforeChaseStart` short; retail's class cast
`0x8269B3D0` in the flee scan not read).

**Verification.** skate-game `a_dropped_phone_stops_the_talk_and_the_unlink_stops_the_channel_once`. Not play-checked.

### Ped attack throw at the skater

**Problem.** An angry ped holding a can (`pedestrian_wanttothrowhandprop.xml`: want `throwhandprop`, `CanAttackThrowHandProp`,
`IsFacingWantTarget angle=120 distance=20`) reached `ThrowHandPropAtWantTarget`, which was not ported: the ped stood
facing the skater and never threw.

**Evidence** [code] (TU3 recomp, reference only; research b25 §5, b92, b93; clip thresholds and names main-read):
- Begin `826A7A58` -> `sub_82E3E960(ped, want target)`: the target and the ped's own body are both predicted 0.8333 s ahead
  (`0x82063BE4`, position + velocity x time); the XZ intercept solve `sub_82E15CD0` at 10.0 m/s (`0x82063BF0`) along the
  dominant axis (strict, so equal |dx| and |dz| have no solution) returns the target's point at the earliest
  non-negative time; with no solution the aim is the PED'S OWN predicted position (b93 B4). The solve's term is
  `v_minor + k * v_major` as read (an exact intercept would subtract; ported as read).
- Then the jitter `sub_82E17508` (3 draws: z, x in [-0.5, 0.5), a length in [0.0, 0.25) m, `0x82165A10` / `0x820C6D98`,
  a horizontal offset along the normalised (x, z)), then +1.0 m up (`0x82139A20` x `0x8231A844`), then `82E3E648(ped, point,
  attack 1)`: holding required, launch speed 10.0, release after 0.8333 s (timer 34).
- The attack clip by the flat angle to the point: below 0.5236 (`0x82063B38`) or above 5.7596 (`0x822F9138`)
  `HandPropAttackThrow`, below 2.618 (`0x822F913C`) `HandPropAttackThrowLeft`, below 3.6652 (`0x822F9140`)
  `HandPropAttackThrow` again (behind), else `HandPropAttackThrowRight` (names `0x820649A4..0x820649E4`).
- Update `826A7AA8`: nothing while held; once released, the want is unset when timer 35 (the flight time) runs out.
- A thrown can hitting the skater is an ordinary prop contact: no hand-prop hit, bail, speech or score code in the DMO
  class, and the skater contact switch (`sub_82BD4A30`, `0x82BD5388`) gives groups 12 / 13 / 14 only the generic
  non-board contact and the region forces (b92 Q2).

**Change.** Core `peds/hand_prop.rs`: `intercept`, `jitter`, `AttackAim`, `attack_point`, `attack_throw_clip`,
`PedBrain::start_attack_throw` (the aim and its 3 RNG draws come first, as retail, from the brain's seeded RNG, so a host
decides them); settings `attack_lead_seconds`, `intercept_epsilon`, `jitter_min` / `jitter_max`, `aim_lift` (retail
defaults; mod keys `ped_brain.attack_throw_*`, see the bin throw section). Brain: op `ThrowHandPropAtWantTarget { want }` (Begin / Update), `ChaseView.velocity`
(target velocities by id) and `own_velocity`. Game: `PedBody.velocity` (last console tick's movement), the velocity map
of peds and players passed to the brain; the attack clips in the ped clip list. The release, flight and link reuse the
bin throw's path above.

**NOT RETAIL YET / open.** The intercept epsilon: the image word at `0x82195100` does not read as a float, 1.19e-7 is
inferred. Retail's jitter draws come from its global generator, ours from the ped's seeded RNG. A zero-length (x, z)
draw keeps the point (retail normalises it, unread).

**The hit (b94, main-checked `82C56BA0`, `skeleton_feedback.rs:208`).** Before this change our skater body never collided
with any prop: the prop layer is only queried by the wheels (`physics.rs:754`) and props take a one-way push from the
skater volumes (`prop_dynamics.rs` `push_from_skater`). Retail's skater contact (`82BD4A30`) handles a DMO like any other
body: the relative normal speed (halved below `SmallObjectMassThreshold` 5.5) goes into the per-bucket force maxima that
the regional wipeout check `82BD88A0` reads against the setup `Wipeout_*SkeletonMaxContact[Arms]` limits; we already port
both (`skate-core/src/player/wipeout/common.rs:19`, `skeleton_body/collision_update.rs:95`). A released hand prop's group
is 12 at or above the small-object mass and 14 below it (`82C56BA0` [code]). So `ped_hand_props::push_hand_prop_proxies`
adds every released hand prop to the skater solve as a solid proxy (box, mass, inertia, velocity, group 12 / 14; ids
`HAND_PROP_PROXY_TAG | body`), the way cars join it (group 8); settings `skater_contact` (mod opt-out), `heavy_group`,
`small_group`. Measured (DownTown, data-gated test): a 0.4 kg can at 10 m/s into a standing skater's chest peaks at region
force 3.49 and does not bail them.

**NOT RETAIL YET / open (hit).** The group-pair filter is not found (b94: probably inside the physics middleware; the
contact recorder `827682B0` has no group test, only same-body and same-owner rejects), so 12 / 14 colliding with the
skater is [inferred, medium-high]; a recomp trace at `0x82BD5388` would prove it. Retail's solve is two-way (inferred from
the reduced-mass factor); ours keeps the prop's reaction in the prop step (the proxy is not owned, its solver reaction is
dropped). Placed (ordinary) DMOs are still not in the skater body contact: their group comes from data (not found), and
adding every prop changes riding into all props on every map, so it waits for the user's go and a regression pass. No
mod event for a prop hitting the skater yet.

**Verification.** skate-core `intercept_solve`, `jitter_is_horizontal_and_bounded`, `attack_throw_clip_by_angle`,
`attack_throw_aims_and_releases`, brain `the_attack_throw_aims_at_the_want_target_and_unsets_the_want_after_the_flight`;
skate-game `hand_prop_proxy_group_by_mass`, data-gated `a_thrown_can_reaches_the_skater_region_forces` (with
`a_traffic_car_knocks_the_skater_down_above_the_contact_limit` unchanged: wipeout at tick 6). Not seen in game yet.

## Angry peds, knocked-down peds on their way to a seat, moved benches and a plugin crash (2026-10-12)

### Problems (play tests, 2026-10-12)
1. An angry ped knocked the skater down, taunted, and chased again the moment its taunt ended, over and over, with no
   new collision (log: `PED_MOOD ... result=skatercollisionchase category=collision` right after `DoTaunt ->
   InitialState`, no `PED_SKATER_CONTACT` since the last takedown).
2. Two peds knocked down several times while walking to a bench never got angry; both then sat down where the bench
   had been before the skater moved it (in the air).
3. The game crashed: "index out of bounds: the len is 15 but the index is 15" at `graph/controller.rs:180`
   (`think_peds`), in the frame props streamed out near an angry ped.

### Root causes [code, the TU3 recomp as reference; graph files from the disc]
- `IsEndChaseReason` (`826AAC80`: the chase component's end reason, vfunc +84, equals op+28) was not ported, so
  EndChase always took its default branch. Retail's `pedestrian_endchase.xml` aggressivecapture branch:
  SendChaseStateMessage end, `IgnoreCollisionsWithWantTarget angrychase`, `UnsetWant angrychase`,
  `SuppressMoodAboutChasee 5.5`, EndChase.
- The mood result's want flag 0x40 (`CE65`, set on the chase, tazeperp and runfromchaser results) was dropped when
  the want was raised: unsetting such a want erases the mood records and the perception entry about its target
  (`sub_82E42A58` -> `sub_82E40FF0`, research b28 / b18). Our collision record (30 s lifetime) outlived the chase.
- (Update 2026-10-20: now modelled as `PedBrain::ignore_collisions_with`, b394.) `ped+5680` (the entity whose contacts the ped ignores) was not modelled: `TakeDownTargetableSuccess` (`826A4E50`)
  and `IgnoreCollisionsWithWantTarget` (`826A8A60`) store the target there; the contact classifier `sub_82E38400`
  resolves it (medium confidence on exactly how the classifier uses it). The takedown's own bumps counted as new
  collisions.
- `PedestrianColliding` (Begin `8269DA00`, End `8269E610`) was not ported and nothing set the collision component's
  needs-to-begin / colliding bytes (+8 / +10, read by `NeedsToBeginColliding` `8269E758` and `IsPedestrianColliding`
  `8269E790`). Retail: a contact sets +8, the graph enters Colliding (also from the Plugin state,
  `pedestrian_plugin.xml`, unless IsPluginCollisionOverride), whose Begin posts the collision mood broadcast
  (`sub_8269D8A8`). Ours posted the mood straight from the contact, and a ped in a plugin stayed busy and kept going.
- Hotpoint seats were fixed at the prop's authored placement (noted NOT RETAIL YET in `ped_plugins::map_props`);
  retail's hotpoints belong to the object.
- The ped's plugin controller was sized for the first plugin graph it ran; when the prop streamed out the ped switched
  to the conversation graph on the same controller.
- A seated ped could not be hit at all: our contact check skipped every ped playing any reaction clip, and the sit
  plays as one. Retail skips a contact only while the ped is already in a collision reaction (`sub_82E38FB8`,
  controller byte +9), and a seated ped's IgnoreStandingCollisions (`brain+3277` bit 0x10, `sit_body.xml`) only drops
  the stumble: a hit above the knock-down speed (3.0) still knocks it down.

### Change
- skate-core `peds/brain.rs`: `Want::forget_on_unset` (flag 0x40) and `PedBrain::raise_want`; `unset_want` of such
  a want forgets the perception entry and requests `ChaseRequest::MoodReset`. New ops `IsEndChaseReason`,
  `IgnoreCollisionsWithWantTarget`, `PedestrianColliding`; `PedBrain::ignore_collisions_with` (also set by
  TakeDownTargetableSuccess), `collision_began`; NeedsToBeginColliding / IsPedestrianColliding now have writers.
- skate-game `living_world/peds.rs`: a reacting contact sets the needs-to-begin and colliding flags; colliding clears
  when the contact reaction ends; the collision mood broadcast is posted when PedestrianColliding begins
  (`PedEvent::Colliding`, log `PED_COLLIDING`); contacts from the ignored entity are dropped (no reaction, no mood).
  Hotpoint seats follow their prop's body every think (`ped_plugins::follow_prop`). The plugin controller is rebuilt
  when the plugin graph changes (`plugin_controller_for`, log `PED_PLUGIN ped=#N graph <old> -> <new>`). The contact
  check runs unless the ped is in a contact reaction and passes IgnoreStandingCollisions to the reaction rule.
- Mods: all of it is driven by the stock graph files and mood tables, which mods already override; no new constants.
  Multiplayer: host-side ped state; the colliding / ignore flags and the broadcast are host decisions.

### Verification
- skate-core `a_captured_chasee_is_forgotten_and_its_contacts_ignored`; skate-game
  `plugin_controller_is_rebuilt_when_the_plugin_graph_changes`, `a_moved_prop_carries_its_seats`,
  `a_seated_ped_ignores_bumps_but_not_a_hard_hit`; data-gated `living_world_ped_data_loads_from_the_export` checks
  `SitAvoidFwd` resolves (12 of 12 sitting animation sets). Living world tests
  86 passed; skate-core 993 passed with only the known upstream `a_moving_group_8_body_...` failure.
- In game: to be play-tested (local test build `hails-local`).

### Open
- Retail clears brain `+3277` bit 0x04 at PedestrianColliding Begin / End and sets `+3196` bit 0x20 at End (meanings
  open, not modelled). How `sub_82E38400` uses `ped+5680` exactly (ignore vs a different contact kind). Answered 2026-10-20 (b394): a contact from that entity is dropped entirely (`82E38738` -> `82E38940` returns 0), never cleared, so a ped that took the skater down ignores that skater's bumps for good; only the physics push stays. Ours matches (laptop session 2026-10-20, #104).
- A bench moved under a seated ped (session `game-20261012-133659`: `PED_PLUGIN_SEAT_MOVED` while ped #6 sat; the
  ped was left in the air). Retail (research `.local/research/peds/b102-moved-plugin-object-unseat.md`, main checked
  `82E3DBE0` and `motiongraph_avoid.xml`): the grab is not refused and the ped keeps its plugin and seat. The owned
  object reporting a move (`82E3DBE0`: the DisableCollisionsWithBehaviourSource source, |v|^2 > 2.5e-5, OwnPluginObject
  bit 0x01 set, `[ped+5712]+1416 == 0`) makes the ped jump off forward (intent via `82E36B58`, motiongraph_avoid SitJump
  -> JumpForward `SitAvoidFwd`, blend 0.1, speech PedestrianAvoidJump 4); the locked waypoint is re-read every evaluation
  (`826AB9C0`), so the graph walks the ped to the seat's new place and sits it again (until the 60 s PluginTimeout).
  Ours: `PedMind::sit_avoid` raised while the seat moves under its owner (`SEAT_PUSH_SPEED_SQ`, the seat's speed stands
  in for the object's vector), the body interrupts the sit cycle with `SitAvoidFwd` (`PedAnimPlayer::interrupt`), and
  the brain's locked waypoint follows the seat. Open: whether `82E3DBE0`'s vector is a velocity or a step, the
  `+1416` gate, the Back avoid (`ReturnToSit`) and who calls `82E3DBE0`.
- Fixed (2026-10-13): a bench moved while its ped was still sitting down stayed sat on forever. User report: "When a
  ped is in the middle of the sitting animation, if you move a bench they will stay sitting." Session
  `hails-local/logs/game-20261013-112342`: second grab at 17:25:21, ped #6 had restarted Sit at tick 2971; the AI left
  the sit (`WaitOutTimer -> TargetWaypointPrecise`) and asked to walk to the new seat for 20 s, but the body stayed at
  [-259.1, 42.97] until `ExitInteraction` (PluginTimeout). Cause: the jump-off only fires from the sitting cycle (right:
  SitJump needs LastState `SittingCyc`), and when the Sit packet ended mid-`Stand2Sit` the motion run was reset without
  releasing the held `SitIdleCyc`, so the body sat forever and locomotion never ran. Retail (`motiongraph_sit.xml`):
  Sit is non-interruptable, `SittingInto` plays `Stand2Sit` out, `SittingCyc` goes to `SittingOut` (`Sit2Stand`) on
  "not HasIntent Sit", then `SitComplete` -> Locomotion. Change: `PluginMotionRun::orphaned` (skate-game
  `living_world/peds.rs`): a packet that ends while its clips play keeps the run, releases the held cycle as soon as
  it is reached and ends with the clips (log `PED_PLUGIN_MOTION ped=#N Sit ended mid-clip, standing up`). Applies to
  every plugin motion with a held cycle (data-driven by `plugin_motion` steps; nothing new for mods to set).
  Test: skate-core `a_sit_ended_during_stand2sit_stands_up_once_seated`.
  Play test (user, 2026-10-13, session `hails-additions/logs/game-20261013-170436`): "I moved a bench as a ped
  started sitting and she properly stood up once she finished sitting."

## Zombie mode: zombies chase the skater (2026-10-13)

### Problem
User (2026-10-13): "I also turned on zombie mode, the peds didn't chase me at all and instead were more interested in
sitting on the benches". Session `hails-additions/logs/game-20261013-170436`: zombie mode on at 23:07:34Z; 22 of 31
peds reached ZombieFollow (they mill around the skater), no `skaterzombiechase` and no chase in the whole log; 9 peds
stayed in seat / conversation plugins.

### Root cause
The chase in zombie mode is not in the ZombieFollow behaviour: it comes from the mood system. Every stock reaction set
has `skaterzombiechase` at priority 1 (category `zombiemode`, instigator `skater`, want `angrychase`, want flag) [data].
Nothing in our engine posted the `zombiemode` mood event, so the result could never fire.

### Evidence (retail code, main-read)
- Poster `sub_82E24B60` (the skater-side broadcaster, also posts skaternoise 5, skaterskitch 4, skaterslam 2): at the
  end of its per-skater loop, when the living-world manager `*(0x83085494)` (set by `sub_826BBC40`) answers vfunc +156
  (the zombie test, b4 / b95), it posts category **19 = zombiemode** on the mood bus (`sub_82B938D8`, message id
  0xF631C8F6, size 80): position = the skater's, instigator = the skater, second = -1 (none), sight factors 1.0 / 1.0
  (`0x8231A844`) [code].
- No timer on that branch: it runs every living-world update, `sub_826BE020` -> `sub_82E24B60` [code], so once per
  console frame (normalised to 30 fps in ours, `mood::BROADCAST_PERIOD`).
- `zombiemode` category: broadcast_continuous, radius 0 (unlimited), magnitude 0.5, lifetime 30 s, priority 1 [data,
  b6]. No stock reaction set has a `zombiemode` prerequisite [data].
- Peds already in a plugin stay there: the producer skips busy peds (`sub_82E3BF70`) and plugin Scatter is off in
  zombie mode (b95). So seated or conversing peds join once their plugin ends; that part is retail.

### Change
- skate-core `living_world/peds/mood.rs`: `category::ZOMBIE_MODE`, `BROADCAST_PERIOD`, `zombie_mode(players)`.
- skate-game `living_world/peds.rs`: while `settings.zombie`, each ped receives one zombiemode post per 1/30 s per
  player through the normal post gate (suppression, prerequisites). Log `PED_ZOMBIE_POST` (accepted / suppressed /
  busy, repeat-limited); the produced chase shows as `PED_MOOD ... result=skaterzombiechase`.
- Session logging: the 2.5 s `LIVING_WORLD peds` readout now ends with `activity:` counts per plugin class
  (`waypoint_sit`, `conversation`, ...), `chasing`, `zombie_follow` and `free`, so a session shows how many peds sit,
  gather or chase at any time.
- Moddability: the zombie flag is the existing setting / Lua toggle; the category, its magnitude and the
  `skaterzombiechase` result and priority are stock table data a mod can override; the post period is a named const
  (console cadence).
- Multiplayer: one post per observer in `player_list` (retail has one local skater).

### Second pass after the user watched the muted test (user: "I was watching the muted test, now make it match retail code")
With the broadcast alone only 2 or 3 of 31 peds chased and none followed: every ped got the chase want at any distance,
most hit `CancelChase` (the player's group used the `global` record, max 5 chasers) and suppressed their mood for 5.5 s,
over and over. Two more retail rules were missing:
- **Zombie chase and takedown records** (research `.local/research/peds/b105-zombie-chase-record.md`, key hash and gate
  main-read): the chase group init `82D967B0` (only caller: group ctor `8258E588`) gives every chase group, the
  skater's and each ped's, the `livingworld_entities_chase` record `zombie` (hash 0x69FABBFD81C0E5A7, `82D96860`) while
  the zombie gate `*(0x83085494)` vfn +156 holds, else `global` (`*(0x830C7200)`, static init `82F8FEB8`) [code]. The
  ped constructor `82E33198` (`82E33FAC`..`82E3402C`) sets the ped's own chase record (`ped+5712`) and takedown table
  (`ped+5728`) to `zombie` under the same gate [code]. Stock `zombie`: max chasers 25, escape 8 m, give up after 1
  takedown, run speed 12, rest 0 [data]. Ours: `ped_mood::GROUP_CHASE_KEY` / `ZOMBIE_KEY`, `PedData::chase_zombie`
  / `takedown_zombie`; every chase group now reads `global` / `zombie` (before: the ped type's record for a ped
  chasee, inferred). Retail picks at construction; ours follows the live setting (open: whether retail rebuilds on a
  mid-session toggle).
- **The vision test on every mood post** (`sub_82E41060` -> `82E27240`, decoded in b18): the event position must be
  inside the category's near sphere (`D16B`) or far cone (`A7EB`, fov, line of sight) times the ped type's
  perception scales. Ours skipped it ("every post is seen"). zombiemode is 5 / 5, so a zombie takes the chase only
  within 5 m, after ZombieFollow brought it in (sprint beyond 15 m, ring 1 to 8 m); the 8 m escape then hands it
  back to ZombieFollow. The test now also applies to presence (10 m sphere / 20 m cone), greeted and the collision
  posts (event origin: the hit ped, inferred), which retail also gates; this lowers greet / converse / spectate
  triggers from peds that cannot see the event. `MoodCategory::near` / `far` from data; scales per type
  (`ped_mood::sight`).
- `OverrideAnimData` (Begin `826A33C8` / End `826A3428`) only swaps the animation record; ours already matches (b105).
- Logging: `PED_CHASE_CANCEL` (chasee, group, cap), `PED_ZOMBIE_POST ... seen=`.

### Verification
- skate-core `the_zombie_broadcast_raises_the_zombie_chase_on_the_skater`, skate-game
  `zombie_chase_and_takedown_records_load_by_key`; full suites: skate-core 995 passed with only the known upstream
  `a_moving_group_8_body_...` failure; skate-game 688 passed, failures: the known `setup::tests::pipelines_accept_...`
  and `retail_menu_movies::tests::retail_menu_movie_set_loads_links_and_runs` ("unsupported menu movie set" on the
  installed menu assets; not touched here).
- Muted zombie runs (Industrial, player idle, `SKATE_ZOMBIE=1`): before 2 to 3 chasing, 0 following; after the
  records and the vision test: 17 on ZombieFollow, chasers rising 1 -> 7 as they come within 5 m, cancels only right
  after a takedown ends the chase group (retail: a group with an end reason takes no new chasers). No panics.

### Sitting and gathering rates (user: "peds seem to be too interested in sitting over just walking around. Peds also
seem to congregate too often")
Research `.local/research/peds/b103-plugin-converse-rates.md` (main re-checked the data and our file:lines): every knob
checked matches retail. Stock `plugin_odds` for `waypoint_sit` are 0.7 to 0.8 for most ped types (adult_male 0.8, bum
0.3) [data]; the offer radius is the prop class field `E2101F2B` (sit 10 m) [data], read at skate-game
`living_world/peds.rs` (offer loop); the offer scan runs every 11 manager ticks at 60 Hz (`826C0058`, b83); a refusal
blocks one prop for 30 s. DownTown has 639 seats among 860 hotpoint props. Conversations start without a roll
whenever two compatible peds meet, limited by the 50 m spacing [data + code].

Two mismatches found and fixed (research `.local/research/peds/b104-plugin-transfer-conditions.md`, constants and calls
main-read):
- `InFrontOfFirstWaypoint` (`8269CF58`, vtable `0x82308844`) [code]: the flat angle between waypoint 0's facing and the
  waypoint-to-ped vector must be under 0.523599 rad (`0x821DBCF0`) or over 5.75959 (`0x822F9114`): a 30 degree cone, no
  distance limit; a near-zero vector (length^2 <= 1e-4) passes. Ours was a 180 degree half-plane, so far more peds
  qualified for every vending machine, ATM, fountain and seat whose descriptor uses it. Now
  `PluginSettings::front_half_angle` (retail default, mod-overridable), skate-core `living_world/peds/plugins.rs`.
- Refusal on leave [code, slot order medium-high]: the 30 s refusal insert `82E40BD0` is called from `8269F248`, the
  `Plugin` behaviour's Update (vtable `0x8230A294`), so retail re-stores it every tick a ped uses the prop and the ped
  cannot be offered that prop for 30 s after leaving. Ours stored it once on attach, so after a use over 30 s the ped
  took the same seat or machine again at once (log: ped #19 left a vending machine at tick 3623 and retook it at 3629).
  Now refreshed each tick while `in_plugin` (skate-game `living_world/peds.rs`, next to the refusal tick).
- `IsNearCrossWalk` (plugin transfer op, `8269CC38`) [code]: true when a crosswalk corner is within the op's `distance`
  attribute; no stock descriptor sets `distance` and the constructor writes no default, so retail most likely never
  passes it (ours: false). Open: zero-fill proof (a one-off hook on `8269CC38` reading `this+36`).
- Tests: skate-core `world_prop_transfer_reads_the_hand_prop` (cone cases).

## Zombie takedowns on a downed skater, NPC skater sim flip-flop (2026-10-13)

### Problems
User (2026-10-13): "First one had zombie mode on. it seems to sort of work but its pretty buggy and your character has
buggy respawns where you can't move." Session `hails-additions/logs/game-20261013-183555`: SKATER_TAKEDOWN, the
wipeout auto-reset respawns the skater in the crowd, the next takedown lands 1 to 3 s later; ped #6 took the skater
down twice within 1.3 s while they were still down.
Second report: "It was REALLY hard to find skaters ... they literally seemed to despawn as soon as I got close to them
... Also the game had MAJOR lag spikes and hitches". Session `game-20261013-183803` (NPC Skater Physics opt-in on).

### Root causes and changes
- **ChaseeIsWipedOut / CanAttemptTakeDownTargetable** (research `.local/research/peds/b106-chasee-wiped-out.md`, slot
  and constant main-read): `ChaseeIsWipedOut` `826ACB70` = the chasee's chase group exists and its vfn +88 holds; for
  the player that is `825948B0`, the phys-out byte 59 (Processed `+2468` bit 18, wiping out) [code]. The same
  `825948B0` is the target's vfn +24 in `CanAttemptTakeDownTargetable` `826ACA08` [code]. Ours had neither (Pending =
  false), so `not ChaseeIsWipedOut` was always true and a downed skater could be taken down again.
  `ChaseeCloseEnoughToSatisfyAggressiveChase` `826AC7B0`: 3D squared distance under 4.0 (`0x82257308`) = 2 m [code];
  with ChaseeIsWipedOut it ends the group (aggressivecapture). Ours: `PedOp::ChaseeIsWipedOut`,
  `PedOp::ChaseeCloseEnoughToSatisfyAggressiveChase` (`BrainSettings::aggressive_capture_distance`, mod-overridable),
  `ChaseView::wiping` fed from `LivingWorldObservers::wiping` (the skater's bit 18). Open: a ped chasee's flag (brain
  `+3277` bit 0x04) is not modelled (false); how long bit 18 stays set through get-up and reset (recomp trace).
- **NPC skater sim flip-flop** (cause of the 0.9 s hitches): a skater blocked by a prop (avoid mode 7) may start the
  physics sim at any distance, but the hand-back sent it to replay beyond 44 m when settled, and the next frame
  restarted it. Log: `NPC_SKATER_SIM #31 jerry_hsu <- replay (distance 79.0 m, blocked by a prop (mode 7))` /
  `-> replay` alternating, 12 starts in the 920 ms hitch second (each start about 75 ms). The hand-back now keeps a
  blocked skater simulated until it is clear (skate-game `living_world/npc_sim.rs`).
- **NPC skaters "despawning" up close** [log]: with NPC Skater Physics on, 59 replay -> physics switches within 40 m
  ended in 26 bails within seconds, and each bail hides the skater for 5 s and respawns it elsewhere on its line
  (`NPC_SKATER_BAIL ... respawn in 5.00 s`, `NPC_SKATER_RESPAWN ... at line node N`). The physics NPCs bail on the
  recorded lines' tricks (e.g. n_heelflip into GrindTipslide). Not fixed here: follow-up (the opt-in stays off by
  default); the sim start cost (~75 ms) is a follow-up too.

### Verification
- skate-core `a_wiping_chasee_is_not_taken_down_and_ends_the_chase_within_2_m`; living-world tests pass (skate-core
  peds 112, skate-game 87).

## Floating peds on coarse navmesh triangles (2026-10-13)
User (2026-10-13): "Additionally I saw floating peds during the test." Session `game-20261013-183555`: 12
`PED_FLOATING` lines, almost all on DownTown navmesh poly 7376 (area 17 = pavement), drawn about 1.7 m above the floor.
Research `.local/research/peds/b107-floating-peds.md` (probe constants main-read): poly 7376 is one 25 m triangle of a
fan (y 31.66 to 32.62) over pavement 1.6 to 1.8 m lower: a coarse but valid retail navmesh. Retail's NavPower mover
probes the ground after the navmesh step (`82EA5F48`, debug bool `probe_for_ground` on by default) and, after a miss,
casts again with 4x the reach (`0x82257308` 4.0) [code]. Ours probed 1.6 m down once and drew the ped at the navmesh
height on a miss. Change: skate-game `living_world/peds.rs` retries with `GROUND_RETRY_FACTOR` (4.0) x the downward
reach; upward stays one step (so overhead geometry is still never taken). Open: which mover byte enables the probe
per ped (`(*(this+556))+109`) and the probe length L (`ctx+16`).

Update 2026-10-19 (session `game-20261019-180444`, University terrace x 12..14, z -127..-220; user: "I thought I saw one
but when i turned back around it looked like they got moved to the floor"). Research b351
(`.local/research/b351-university-floating-strip.md`; main verified our lines): not a conversion gap [data: one
collision surface per logged point, a flat terrace at y 54.5]. The 16 m navmesh triangles span terrace and
embankments, so on the lower half the navmesh sits 0.2 to 0.5 m under the floor; our window reached only 0.2 m up and
the retry widened only downward, so the cast started below the surface and the ped stayed at the navmesh height
(drawn sunk). The `PED_FLOATING` check had the same blind spot (`floor_y=none`). Retail's probe `82EA5F48` casts
symmetrically from point + axis x L to point - axis x L, retrying at 4L [code, recomp]. Change: the retry widens both
ends by `GROUND_RETRY_FACTOR`; the floating check also looks 2 m up, so a sunk ped logs a negative `gap`. Open:
retail's L for peds and the geometry filter of the cast (a recomp hook at `82EA5F48` with a short teleport drop at
(12.9, 54.5, -220) would settle it). All maps counted by b358 (`.local/research/b358-navmesh-vs-floor-all-maps.md`,
simulated casts on the extracted collision) [data]: walkable navmesh (pavement 0x11, road 0xA1) where both casts still
miss a floor above: University 72 m2, DownTown 19,910 m2, Industrial 1,689 m2, almost all on road triangles of about
2,000 m2 over sloped streets (floor 0.8 to 4.3 m above, e.g. DownTown polys 32918 / 32922 / 32923 at x 6..84,
z 1314..1379); on pavement alone 72 / 176 / 8 m2. An upward retry of 1.6 m would leave 0 / 7,516 / 364 m2, 3.2 m
0 / 589 / 0 m2 with the first overhead hits (DownTown polys 16685/16686, 32551/32552). Not changed: retail's L for peds
decides it (recomp trace of `82EA5F48` pending, needs the user not gaming).
Where retail draws the ped (b360, `.local/research/b360-ped-drawn-height.md`, partial) [code]: the probe runs once per
path-node advance (`sub_82E96B88`), at the next node point on the next polygon's plane and along that polygon's
normal; a hit becomes the target of a NavPower path follower (vtable `0x8232E4F0`) whose position the bot reports, a
miss leaves the plane point. The ped sync `82E37108` keeps the ped's own y (`vrlimi128 v123,v63,4,0`, main verified),
so the bot's probed y does not reach the ped there; who writes the ped's world matrix (ped vtable `0x8232BE80`
slot +0x20, `82E221B0`) decides the drawn y and is still open (a hook on `82E221B0` with the `82EA5F48` run). Ours
stays a render-only per-frame cast, a stand-in with probably the same visible result.

## Peds give up a chase they cannot reach (2026-10-13)
User (2026-10-13): "the ped doesn't give up when attacking the player if they fall into the water, which is something
we have seen in the recomp where being unable to reach the player causes them to give up".
Research `.local/research/peds/b108-unreachable-target.md` (main-read: `826AD640` bit, `82E31D58` constant and result
codes): `CannotReachChaseTarget` (`826AD640`) reads brain `+3280` bit 0x08 [code]; its only writer is the intercept goal
provider's result (`82E31D58`): for result codes 0, 1 and 3 it sets the bit when the ped is within 5 m flat of the
chasee (25.0 at `0x8209994C`) and clears it beyond [code]. The stock graph (`pedestrian_chasetargetunreachable.xml`):
stop, warn, `SetTargetUnreachableTimer` (`826A8810`: the chase record's `unreachable_timer`, 5 s for peds), then
`ChaserEndChase lostinterest` and `SuppressMoodAboutChasee 10` (or a taze where allowed). Ours: the three ops ported
(`CanTazeUnreachableTargets` false until its writer is found); the host sets the bit when the chasee is off the
navmesh within `unreachable_radius` (5 m) and clears it when the chasee is back on it [inferred mapping of retail's
result codes]. Log `PED_UNREACHABLE`. Test `an_unreachable_chasee_starts_the_record_wait`. Not checked in game yet.
Related, bench knocked into water: the ped gave up after ~67 s (tick 10251 to 14295, `ExitInteraction`), retail's
60 s PluginTimeout; retail has no reachability check for plugins (b108) [data].

Update 2026-10-19 (PR #52 next item 2; user 2026-10-15: "I angered a ped and he still attacked me after I fell in the
water and was unreachable."). Research b302 (`.local/research/b302-chase-target-in-water.md`; main verified the intercept
goal vtable `0x8232C8A8` slot 0x18 = `82E31D58` and our lines):
- Retail [code]: no water rule. The report comes from the mover update `82E2CB08`: every 10 mover updates it snaps the
  ped and its current destination onto the navmesh (`82C465A8`, 0.5 m window) and tests that both have a polygon and
  are connected (`82C464F0`). Every unreachable result (codes 0, 2, 3, 4; and 1 with a high area class, `82C47D60`)
  posts an event; a reachable one posts none. The intercept goal's handler `82E31D58` then sets the bit to "within 5 m
  flat". So the bit only changes on an unreachable report, and a chasee climbing back onto the mesh does not end the wait.
- Ours before: tested only the chasee off the mesh, every tick, and cleared the bit as soon as the chasee was back on the
  mesh. The 2026-10-15 log (`.local/session-logs/hails-local-2026-10-16/game-20261015-133314.stderr.log`): #185 set the bit and
  gave up 5.05 s later (as designed); #189 set it at 19:47:58.87 and cleared it 0.85 s later with `near=false` (the
  swimmer more than 5 m away, which retail does too) and kept chasing; the skater left the water at 19:48:04.57 and was
  taken down on land at 19:48:07.96. So this case already followed retail's 5 m rule; what the port changes is the
  cadence, what counts as unreachable and the clear-on-reachable.
- Change: `chase::reach_failed` (ped or destination off the mesh, or not connected) and `chase::apply_reach_report`
  (only a failed report rewrites the bit) in skate-core; the game's intercept arm checks every
  `reach_check_seconds` (10 / 30 s: 10 mover updates at the console's 30 fps) against its destination (the intercept
  goal; the chasee while no goal can be made, as the recomp suggests the off-mesh point is not snapped, unproven).
  Ours: EndChase and a new chasee clear the bit (retail leaves it to the next report; avoids a stale wait). Mod keys
  `ped_brain.unreachable_radius` and `ped_brain.reach_check_seconds`. `PED_UNREACHABLE` now logs `dist`, `at` and
  `chasee_at`.
- Tests: `reach_reports_fail_off_the_mesh_and_across_islands_and_only_they_rewrite_the_bit` (chase.rs),
  `end_chase_resets_the_end_reason_and_unsets_angrychase` (now also the bit). skate-core peds 148, skate-game
  living_world 110 passed. Not play-tested.
- Destination (research b306, `.local/research/b306-intercept-destination-off-mesh.md`; main checked the tail
  `loc_826A3F08` in `skate3_recomp.27.cpp`: `vfn168(speed)`, `vfn136(0.5)`, `vfn4(r1+128, 1)` on every path) [code]:
  InterceptChasee Update `826A3BF0` hands the mover the chasee's raw position unless the ped and the chasee are
  reachable (`82C465A8`) and the solver finds an intercept; then the lead point, projected onto the mesh where it can be
  (`82C460E0`), else raw. Nothing on the way to the bot (`82E2CFC0` -> `82C47010` -> `82E95C00`, `bot+0x268`) snaps
  it, so a swimmer fails the check. A path request to an off-mesh point paths to the nearest point of the flooded region, with no distance limit (`82E957F8` -> `82933E18`, snap `8292F9F0`; research b733, static read, corrects b306).
  Ours before kept the last on-mesh goal (`chase_goal`) when no goal could be made, so a ped that had a goal before
  the skater jumped in tested that stale bank point and never set the bit; and it made goals by line of sight
  (`clear_line`), not reachability. Changed: the destination is rebuilt every think from that table (reachability
  gate, raw chasee otherwise; no record = no intercept = the chasee itself), the route follows it only while it is on
  the mesh. skate-game unit tests 814 passed.
- Open: the area-class case; whether Pursue / Block use the same goal object. Not ported: routing to the nearest bank point (ours keeps the old route for an off-mesh destination, `peds.rs` InterceptChasee); `locate_nearest` is a linear scan, so per-think calls need an optimisation pass first.

## A ped stuck in front of the bench it wants to sit on (2026-10-14)

**Problem.** In a play session a ped took a bench seat, walked up and stood 1.35 m short for a minute (PED_BLOCKED by
the seat's own bench, AD_Bench_DMO_1009, University), then kept walking into it after giving up.

**Root cause / retail.** The seat waypoint lies inside the bench's obstacle footprint. Retail's sit plugin
(`state/livingworldentities/pedestrian/plugin/sit.xml`) starts with `DisableCollisionsWithBehaviourSource`
(826A7378 / 826A7410; 82E3DB68 -> ped+5916): the ped ignores its plugin's own prop while it approaches. Ours set the
brain flag `ignore_source_collision` but nothing read it.

**Change.** skate-core `Obstacles::resolve_step_ignoring` / `step_ok_ignoring` (one obstacle id left out);
skate-game `peds.rs` passes the plugin prop's body id (the plugin prop id's low 32 bits) while the flag is set.
Driven by the behaviour, so mod plugin graphs using it get it too.

**Open.** A ped that left the plugin on its 60 s timeout (that part works) kept walking into the bench; not traced.
Whether retail's ped+5916 also reaches the NavPower planner / avoider is not decoded.

**Verification.** skate-core `a_ped_ignoring_its_plugin_prop_steps_into_it_and_nothing_else`; skate-core peds 114,
skate-game 693 (known failures only). Play test pending.

## Diagnostics: still peds report what they are doing (2026-10-16)

**Problem.** The user saw small crowds of peds standing next to a pair in conversation ("some congregate, but far
too many decide to join and it becomes little crowds of peds" / "they are all standing next to a couple of peds in
conversation for some reason"). Session `game-20261015-220114` could not show why: conversations never had more than
2 members, only 12 of 154 greets targeted a conversation member, and the log has no positions for peds standing still.

**Change.** `PED_STILL` (skate-game `living_world/peds.rs`, `log_still_ped`): a ped that stays within 0.1 m for 5 s
(300 ticks on the 60 Hz world clock) logs its position with its graph state, plugin graph / state / prop, conversation
membership and conversations within 5 m, animation (locomotion, clip, clip time, channel clip, taunt, plugin motion),
navigation (target, corner, waiting, route, poly, blocked), heading, velocity, flee / chase goals, mood store and the
whole brain (minus its RNG). It repeats every 5 s while the ped stays; a report equal to the previous one logs a short
"unchanged" line. Reports queue: at most one full report per tick (`STILL_REPORTS_PER_TICK`), so many stuck peds do
not all format in the same frame (user: "make them queue up the self report so many peds stuck don't cause spikes").

**Verification.** skate-game ped tests pass (22). Needs a user session to explain the crowds.

**Walking in place: `PED_NAV_STALL` (2026-10-22, USER-ASKS 141 i, research b511).** Two peds stuck at a raised
walkway in the laptop session 2026-10-20 10:13 crept about 0.3 m in two minutes, so they kept leaving the 0.1 m still
radius and logged one `PED_STILL` in total; the log could not show why they did not reach a target 3 m away. Now a ped
with a nav target that stays within 0.5 m (`STALL_RADIUS`) for 5 s logs `PED_NAV_STALL` with the same full report
(same queue, same "unchanged" short form), unless it already counts as still. Test
`a_ped_walking_in_place_is_due_for_a_stall_report`. The cause itself stays open (b511: the 2-tick brain flip between
InitialState and PedestrianDoStartConversation restarting locomotion, or the body clamped at poly 6904's edge); the
next session with stuck peds decides it.

Refused conversation starts now log at INFO (`PED_CONVERSATION start ped=#n with=... refused: <reason> held=<n>`, and the
"no waypoints" line, once per 5 s per ped and reason with the held-back count): in that session both peds retried every
2 ticks with no line at all.

## Standing pros: where the challenge hosts stand (placement export, 2026-10-16)

**Problem.** The pros and filmers who stand at challenge spots in retail are challenge hosts (research b96), and our
engine places none. Their spawn position was open.

**Root cause / evidence** (research b129 `.local/research/peds/b129-standing-pros-placement.md`, b130
`b130-marquee-skeleton.md`; main's checks marked):
- Each challenge record (class `A4F35B159D7432B5`) has a `tGlobalSpawnCharacterInfo` array (field
  `555C505A36C3353A`, 40-byte elements). Element `+24` is a text pointer to a **locator name**, not an id, e.g.
  `otl_univ_04_challengelocator_01` [data]. `sub_825543E0` reads it, hashes it (`sub_8296EA10`) and finds it in the
  LocationManager registry (`sub_82910CE8`); a hit goes to `sub_825547F0` -> `sub_82911008` (the record's `+48`, the
  matrix's translation row), a miss to a pending list (`sub_82557228`) [code, main verified the call chain].
- The locators are the same `global_locators/*.rx2` records our teleports already read (`teleports.location_records`).
  Exact-name matching places 77 of 80 hosts (Industrial 26, University 23, DownTown 22, MaloofMoneyCup 3,
  BlackBoxPark 2, SkateSchool 1); `ftl_univ_01_kalis_locator_01`, `photo_dwtn_02_startlocator_01` and
  `street_dtwn_01_challengelocator_01` exist in no locator file, so retail keeps them pending (no host there,
  inferred) [data, main ran the export 2026-10-16].
- Some challenges have two elements (two people or two spots); `+28` is 0 / 1 / 2 (2 on every photo challenge),
  meaning open.
- Model: the marquee skeleton (`MarqueeSkeletonPres`, 133 bones) shares bones 0 to 24 by name and index with the ped
  rig, so our name binding (`present_ped_looks`, `crates/skate-game/src/living_world/peds.rs:1134`) covers the body;
  retail ships no remap table. The photographer graph names `PHOT_SHOOT_REVIEW_N_0_N`, which no anim bank contains
  [data, main checked the graph line].

**Change (step 1 of the port).** `tools/asset_pipeline/marquee_hosts.py`: the `livingworld` setup group writes
`living_world/marquee_hosts.json` = per challenge and element {challenge, element, entity key, entity name (the key is
`hash64` of a `livingworld_entities` name: all 80 resolve, 23 people), locator, flag, map, matrix}; unplaced names keep
`matrix: null`. Test `tools/asset_pipeline/test_marquee_hosts.py`. Not yet read by the
game.

**Next.** Spawn the hosts from that list on the marquee graph (ops research b131), offline free skate, with a mod
entry point to add, move or remove hosts (stable id = challenge key + element index; the entity key is not
unique: one pro hosts 12 challenges).

**Open.** When retail spawns / despawns a host (who calls `sub_82553DD0`, the `IgnorePrereqForLivingWorld` filter,
distance), the heading (row 2 of the matrix, inferred), the `+28` flag, the clear-area box `sub_82554698` builds
around a host.

## IsFacingSkater: a half angle through a cosine, and its distance (2026-10-16)

**Problem.** Our `IsFacingSkater` compared the facing dot product with `FOVAngle` itself and ignored `Distance`.
Stock graphs (0.8) got a 37 degree cone instead of 46, and the marquee graph's `FOVAngle="1.4"` could never be
true, so a standing pro would never leave HoldAtSpawn.

**Root cause / evidence** (research b131 `.local/research/peds/b131-marquee-ops.md`; main verified 2026-10-16):
the condition's evaluate `sub_826A9E60` loads op `+32` (`FOVAngle`, default 0.8) into v1 and calls `sub_82473930`,
a range-reduced even polynomial with constant term 1 (a cosine), then tests the flat dot product against the
result; op `+48` (`Distance`, default 0.0) compared with 0.0 skips the range test, else the flat distance must be
below it [code].

**Change.** `crates/skate-core/src/living_world/peds/brain.rs`: `PedOp::IsFacingSkater { fov, distance }`, true when
`dot > cos(fov)` and (`distance == 0` or flat distance < `distance`). Test
`is_facing_skater_takes_the_cosine_of_its_half_angle_and_a_flat_distance`. `IsFacingWaypointOrientation` already
compares the angle (`acos(dot) <= fov`) and is unchanged.

**Verification.** skate-core ped tests pass (115 + the new one). Play-test: peds that react when the skater is in
front of them now do so over a slightly wider cone.

## Peds are solid: the skater collides with them and can bail (2026-10-16)

**Problem.** Session 10: "peds should have similar physics as npc skaters in the sense that they should move as your bump into them, whereas at the moment you go through them and they 'react' to it after the fact. they also can cause you to bail in retail if you run into them going fast or doing a trick."

**Root cause** [code, research b138; main checked our side]. Retail peds are bodies in collision group 11 inside the
skater's skeleton contact solve (`sub_82BD4A30`, group switch at `0x82BD5388`: 5 skater, 8 vehicle, 11 pedestrian).
The solver pushes the two apart, the group-11 force is kept (`+4060`), and the contact also feeds the per-region
strongest contact that the wipeout checks read, so a hard hit bails the skater through the ordinary thresholds. Our
skeleton side already had group 11 (`maximum_group_11_force`), but no ped ever added a body, so the skater rolled
through and only the distance test (`peds::skater_hit`) started the ped's reaction.
Research b144 [code, main verified the groups in the recomp]: each ped's collision object builds two collision bodies
(`sub_82E268C8`, ctor `sub_82E0E2A0`): A in group 9 and B in group 11 (the one the skater meets). Agent-derived, not
re-checked: B has two parts with finite masses (`sub_82AE7770`), posed from bones, and each body can be removed on its
own (`sub_82E0E730`). Shapes, sizes and masses are data (collision asset, vault), still to be traced.

**Change (step 1).** `living_world/ped_body.rs`: every ped not in a collision reaction adds one upright capsule to the
skater solve, group 11, the navmesh agent's radius and height ([data] NavPower header 0.35 / 1.6 m), moving at its step
velocity, id `PED_PROXY_TAG | ped id`, appended after the released hand props. The ped's own reaction (knock-down /
stumble, `sub_82E38FB8` kind 5) is unchanged. Mod tuning `living_world.ped_body {enabled, mass, radius, height}`
(validated, reset on mod disable).

**Change (step 1b, retail parts).** Recomp trace b146 (`hooks_pedbody.cpp`, Aletown, 41 peds) and research b147
(`sub_82E35838`, bone table `sub_82E33070` at 0x82FC9F14, main checked the table and Spine1): body B is three capsules
posed every frame from the skeleton: part 0 radius 0.20 half length 0.575 10 kg at Hips along Hips -> Head; parts 1 / 2
radius 0.08 half length 0.20 20 kg each, halfway between LeftForeArm / RightForeArm and their hand along the forearm;
all moved ahead by the ped's velocity x 1/60 s. `ped_body.rs` poses them from `peds::ped_globals` and the ped root
(peds within 12 m of an observer, ours); tuning `ped_body {enabled, parts[{from, to, centre, radius, half_length, mass}],
look_ahead, range}`. Data test `the_retail_parts_find_their_bones_in_the_ped_rig` (reference pose: hips 0.92 m, head
1.59 m); `a_ped_body_is_solid_to_the_skater` with the retail part 0: 1.4 m/s force 1.17, no wipeout; 9 m/s 8.28, wipeout
tick 8.

**Earlier (step 1, superseded).** One capsule instead of body B's parts; mass 75 kg (stand-in, finite so the solve gives the ped
an impulse); no body while the ped is in a collision reaction (research b145 looks for retail's remove / add calls); the
ped is not yet displaced by the push (b144 found no root write-back; open).

**Tests.** `the_body_is_an_upright_capsule_on_the_ped_in_group_11`, `ped_body_is_mod_reachable_and_reset_on_disable`,
data test `a_ped_body_is_solid_to_the_skater` (DownTown, standing skater): a ped closing at 1.4 m/s reaches group-11
force 1.30 and no wipeout; at 9 m/s 20.61 and a wipeout at tick 8 (the ordinary region thresholds).

Answered 2026-10-19 (b368, `.local/research/b368-ped-push-aside.md`, after b217) [code]: retail writes nothing from
the solve back to the ped or its NavPower bot (body B is keyframed from the bones every frame; the contact callback
`82E38FB8` and `8269E170` only pick the reaction and its direction; the bot is resynced to the ped, never the other
way). The ped is moved by the reaction clip's root motion [trace b217: stumble 0.25 to 1.1 m over 0.7 to 1.2 s,
knock-down 1.6 m over ~1.06 s], which ours already does (`peds.rs`, the walker applies the clip's root translation).
**Open.** The b144 trace (per-part shapes and masses, remove / add around a knock-down);
a session (does it feel solid, do fast and mid-trick hits bail, does the ped's reaction still fire).

## Speech: the start delay and the stream gap (2026-10-16)

**Problem** [code]. The speech tuning's first two words (setup exports them as `unknown_0` / `unknown_4`) were read by
nobody: every line started the frame it was accepted, and a stream took the next line at once.

**Retail** [code: research `speech-stream-start`, main read the start in Ghidra pseudocode]. `sub_82C5DC80` starts a
line on its stream at once (active, speaking, interrupt rules, the queue waits) and schedules the voice at the library
clock (a double) + the event's tuning `+0` (`sub_824823C0` reads the float); scripted dialogue holds a stream with
the same clock + 1000.0 instead. `sub_82C5DA88` stores tuning `+4` on the stream after a start; `sub_82C5CC90` counts it
down by the frame's dt only while the stream is idle, and the stream takes no new request until it runs out. Unit:
seconds [inferred, high: the clock is a double time, the gap counts down by dt, 1000.0 as "never"]. Installed data:
`+0` non-zero for 18 of 73 living-world events (up to 3.0 s), 43 of 64 main-cast (3.0), 31 of 46 (4.5) and 39 of 64
announcer (1.5); `+4` up to 10 s.

**Change.** `EventTuning::start_delay` / `stream_gap` (seconds, from `unknown_0` / `unknown_4`; a mod's tuning may
write `start_delay` / `stream_gap`). The speech host turns them into console frames (`evaluation_dt`) on the request;
`SpeechPlayer` holds the line's stream with no voice for `start_delay` frames (the cut and speaker checks run as for a
playing line), and keeps a stream that started a line closed to new lines until it has been idle `gap` frames
(`available()` counts it busy). Test `the_start_delay_holds_the_voice_and_the_gap_holds_the_stream`.

**Open.** A recomp check of request-to-voice timing for an event with a non-zero `+0` (the speech sessions' traces).

## Ghidra pass 1: knock-downs, mood escalation, chase groups, the taze capture (2026-10-16)

The user asked for Ghidra fixes across the living world to get it closer to retail (plan
`.claude/todo/living-world-ghidra-pass.md`). This pass compared the ped contact, aggression, chase and taze code with
the TU3 pseudocode (Ghidra with the Xenon360 extension) and the generated recomp code. Research: b152 (aggression),
b153 (taze), b154 (chase), b155 (speech, ported later); main re-read every address below before porting it.

### Problems and root causes [code]
- **A warned ped never escalated to a chase.** The mood records' `7FF7` test ("outstanding reactions about the
  instigator", [1, 1000] on the stock `collisionchase` results) counted the wants the ped still held. Retail counts on
  the instigator's perception entry (`+68`): `82E42A58` adds one when a set want (0x80) without flag 0x40 is unset, so
  the count is the number of reactions the ped has finished about that entity. Once a warn ended, ours read 0 and the
  chase results could never pass.
- **The perception entry that holds that count was not made.** A passed mood result calls
  `82E42868(brain, target, 30.0)` for each want (`sub_82E41B98`, constant `0x820D4924`): find or create the entry and
  keep it known for at least 30 s. `82E42868` keeps the larger memory; ours overwrote it.
- **Mood prerequisites.** `sub_82BFEA60` (each item about the ped itself): kind 2 is OwnPluginObject (`brain+3277`
  bit 0x01), kind 4 "the ped has a chasee", kind 3 the distance from the ped to the record's position (`+48`, not the
  instigator's live position); comparators 4 and up fail. Ours answered false for kinds 2 and 4, measured kind 3 to
  the instigator, and passed unknown comparators.
- **Every post runs the prerequisites** (`sub_82E41060`): a contact is posted as `collision`; when the prerequisites
  refuse it and the hit ped is another ped, it is retried as `nearbycollision`. Ours chose the category by "was I hit"
  and skipped the prerequisites for collisions and greets. Stock effect: the security sets have no kind 6 item, so a
  guard who sees another ped hit gets `collision`, not `nearbycollision`.
- **The roll cooldown** starts only with a real roll (`|p|` above `0x82063A40`, inside the roll branch); ours always
  started it. Stock records all have p > 0, so this matters to mods.
- **The hand-prop gate** (`072D`, `brain+3278` bit 0x02) was always fed false; the bit is set while a hand prop is
  attached (`82E3EC60`) and cleared on release (`82E3EBE0`, `82E3FAE0`), so the six `throw*` results never passed.
- **The collision instigator** was always the first player. Retail stores the toucher in the brain's contact record
  (`+3184`); ours now carries it on `PedEvent::Hit` (and the mod event `ped_hit` as `toucher`).
- **DisableHeavyCollision** (`brain+3277` bit 0x20) was tracked but not read: `sub_82E38FB8` clears its knock-down
  test while the bit is set, so a hard hit only makes the ped stumble.
- **IsEndChaseReason** (`826AAC80` -> GetEndReason `82E3D988`): no group answers -1, an ending group's reason wins over
  the ped's own. Ours put the ped's own first and answered without a group, so a chaser could take the lostinterest
  branch while its group ended with aggressivecapture.
- **AddChaser** (`82D96C38`) refuses a join when the group already holds MaxChasers (vfn16); ours checked only the
  capacity of 25.
- **GiveUpPrimary** (`82D97108`) swaps the primary with the next chaser and moves the old primary to the back
  (erase + push_back), so the primary goes round a group of three or more; ours swapped the first two only.
- **TazeWantTarget**: Begin (`826A8398`) sets speech 66 and timer 39 only when the want's target resolves; the update
  (`826A8420`) hits once, then ends the target's chase group with reason 1 (aggressivecapture, group vfn24(1)) while it
  has chasers. Ours spoke and timed without a target and never ended the chase after a taze.

### Changes
- `skate-core` `peds/perception.rs`: `Perception::reacted` (`+68`); `know` keeps the larger memory.
- `skate-core` `peds/brain.rs`: `raise_mood_want` (want + 30 s memory, `BrainSettings::mood_want_memory_seconds`),
  `unset_want` counts finished reactions and forgets a flag 0x40 target only when the want was set; IsEndChaseReason;
  TazeWantTarget's target gate and the capture group end.
- `skate-core` `peds/mood.rs`: prerequisite kinds 2 / 3 / 4, comparators, the roll cooldown and its epsilon;
  `MoodContext::own_plugin_object` / `chasing`.
- `skate-core` `peds/chase.rs`: `ChaseGroup::add(.., max_chasers)`, `give_up_primary` rotation.
- `skate-game` `living_world/peds.rs`: `contact_rules` (DisableHeavyCollision), the toucher on `PedEvent::Hit`, the
  outstanding count from the perception entry, prerequisites on every post with the `nearbycollision` retry, the
  hand-prop bit, the group join with the chasee's max.
- Mods: world tuning `ped_brain.mood_want_memory_seconds` (0..=100, retail 30.0); mod event `ped_hit.toucher`.

### Verification
- New tests: `unset_mood_wants_count_finished_reactions_on_the_perception_entry`,
  `a_taze_needs_a_resolved_target_and_ends_the_targets_chase_with_a_capture`,
  `prerequisites_read_the_peds_own_state_and_the_event_position`,
  `disable_heavy_collision_turns_knock_downs_into_stumbles`, and the group test extended (full group, rotation).
  The presence test now posts the far event 13 m away instead of moving the instigator (retail measures to the event).
- skate-core living_world::peds 122 passed; skate-game 714 passed (1 known failure,
  `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`); skate-mods 106 + 3 passed
  (1 known failure, `skyline_every_component_is_real_and_drives_through_ground_contact`).
- Not yet played: needs a session (bump a ped, wait for the warn to end, bump again: retail chases).

### Open
- Prerequisite kinds 1 (`0x8269A888` vfn16), 5 (`0x8269A950` vfn8, probably an area test), 7 (the chasee's
  `0x8258E778` vfn12) and 8 (`brain+540` bit 0x80, want slot 10) are still false.
- The perception list's eviction rule (`82E441C8`) and the per (ped type, instigator type) category mask
  (`82BFE960`) are not read; the producer gate `brain+3278` bit 0x08 has no known writer.
- Ped on ped contact (kind 5 with a ped toucher: 6.0 thresholds, zombie mode drops the reaction; a toucher whose
  chaser component answers vfn92, `brain+3276` bit 0x02, forces a knock-down; the setter is chaser vfn88, its caller
  not found) and NPC skaters as touchers are not ported; asked the user whether retail does both.
- GiveUpPrimary's named new primary and the old primary's rest time (vfn48 / vfn52) are not ported.
- The tazer's draw states (b153 M1 to M3: drawn when the draw clip nears its end, the holster clip, the `tazr` prop and
  the Tazer motion messages) wait for the animation pass (item 6).

## NPC skaters and other peds knock peds over (2026-10-16)

### Problem
Only the players could knock a ped down or make it stumble. Retail's contact callback `sub_82E38FB8` reacts to any
`IActor` owner (kind 5, skater thresholds 3.0 / 3.0) and to another ped (the toucher casts to `IPedestrian`,
`sub_826C2C70`). The user confirmed both for retail (2026-10-16): asked whether NPC skaters knock peds over, "Yes they
can"; asked whether peds stumble when another ped runs into them, "yes i think so?".

### Retail [code]
- Ped toucher (`sub_82E38FB8`, the block after the direction): the ped thresholds `081F978F8706D77B` /
  `5E566360FDC5FDD1` (6.0 / 6.0, exported as `knockdown_speed_flagged_a` / `_b`) replace the skater ones. Under the
  zombie cheat (`0x83085494` vfn156) a ped's touch gives neither a knock-down nor a stumble. A toucher whose chaser
  component answers vfn92 (`82E3D968`: its `brain+3276` bit 0x02) knocks the ped down whenever the set allows it and
  counts on `brain+3248`; that bit's setter is chaser vfn88 (`82E3D920`, only in vtables `0x8232C198` / `0x8232CEA0`),
  whose caller was not found.
- The classifier `sub_82E38400` lets a ped toucher through after a centre distance test (R = the two vfn28 values
  summed times 2.0, `0x82060C50`), and drops it when the touched ped's state `+0xEC` is 2 and two component checks are
  clear (meaning open).

### Change
- `skate-core` `peds/skater_contact.rs`: `CollisionRules::ped_knockdown_speed` (data, `skate-data` reads the export)
  and `ped_reaction` (the ped toucher rule above; `forced` is passed false until the setter is found).
- `skate-core` `peds/wander.rs`: `separation_blocker` names the ped that refuses a step.
- `skate-game` `living_world/peds.rs`: the skater touchers are the players then the NPC skaters (their replay sample's
  position and velocity) in id order; a step refused by another ped is a body contact (`PedContact`), and both peds
  take the other as toucher on their next step (`ped_hit`). The log line `PED_SKATER_CONTACT` names the toucher
  (`by=`), and `PedEvent::Hit::toucher` carries it into the mood broadcast.
- NOT RETAIL YET: an NPC skater is the same 0.35 m cylinder as the player; a ped's touch speed is the closing speed
  along the line between the two peds (retail: each body's speed after the Havok solve).

### Verification
- Tests: `a_ped_touching_a_ped_uses_the_ped_thresholds_and_zombie_mode_drops_it` (core),
  `a_ped_walking_into_a_ped_makes_it_stumble_unless_zombie_mode` (game). Not play-tested yet: watch NPC skaters ride
  through a crowd, and a chaser running past peds.

### Open
- The forcing bit's setter (chaser vfn88 caller) and the classifier's state-2 exclusion.
- Whether retail NPC skaters' bodies use the player's capsule or their own (the ped hits recomp session would show it).

## Peds use their hand props: eat, drink, phone calls, reading, litter (2026-10-16)

### Problem
Peds carried their hand props but never used them, so the phone ring, the call lines and the litter line (speech 49,
48, 50, 61; the audio side was ported, research b155 item 2) never happened.

### Retail [code + data] (research b164; main re-read `82E3F818`, `82E1FCD8`, `82E3F1D8`, `82E3F470` and the strings)
- The attach (`82E3DE18`) takes the entity's usage of the prop (`livingworld_entities` `handprops`: {handprop, usage})
  and that usage's action list (`livingworld_handprops_usagecharacteristics` `actions`: {action, weight, min, max});
  a non-empty list sets has-actions (`+0xCCF` 0x20) and picks.
- The pick (`82E3F818`): `rand % 100 + 1` against the running weight sum x 100 over at most 16 entries; no pick clears
  has-actions for good. It sets the action (`+0xCBC`) and timer 31 = min + u (max - min), u = `rand x 2^-32`.
- The start (`82E3F1D8`) when timer 31 runs out: 1 HandPropEat, 2 HandPropDrink, 3 HandPropPhoneAnswer + speech 49,
  4 HandPropReadInto, 5 HandPropLitter + speech 61 and the drop after timer 34 (0.91667 s); 0 does nothing (and is
  never picked again: `answercellphonerarely` is 80 % action 0); blends 0.2.
- The update (`82E3F470`): the phone talks (HandPropPhoneTalk, speech 48) for 7.5 s, says 50, talks 3.5 s more and
  hangs up; a read cycles 2.0 s and reads out; any other action waits for its clip to end (0.2 s before), returns to
  the carry pose and picks again. DisallowHandPropActions holds the start back 15 s at a time.

### Change
- `skate-core` `peds/hand_prop.rs`: `HandPropAction`, `HandPropActionSettings` (every time and speech value, retail
  defaults, under `HandPropSettings::actions`), `attach_hand_prop_actions`, `pick_hand_prop_action`,
  `update_hand_prop_action` (returns the channel cue) and `update_litter`; `HandProp` holds the list and the bits.
- `skate-core` `peds/anim.rs`: the nine clip names and `channel_remaining` (the clip-time query, [inferred] time left).
- `skate-game`: `HandPropData` loads the usages and each entity's usage per prop; `sync_hand_props` attaches, runs the
  update (log `PED_HAND_PROP_ACTION`) and releases a littered prop through the existing release; `advance_peds` plays a
  new cue once and asks again for the talk / read cycles when their clip ends early (retail asks every frame).

### Verification
- Tests: `a_phone_call_rings_talks_ends_and_picks_again`, `litter_drops_the_prop_and_action_zero_is_terminal`,
  `disallowed_actions_wait_fifteen_seconds`; the data test `every_census_entity_resolves_a_look_and_animation` now
  also checks the nine clips in the default set (all present). skate-core peds 127, skate-game 715 pass. Not
  play-tested yet.

### Open
- What a reaction, a sit or a conversation does to a running action (nothing in `82E3F470` cancels one); the litter
  drop vector (ours zero); whether retail's channel ignores a repeated request; the usage entry's `f32_24` / `f32_28`.

## Ped turns: retail's turn buckets, standing and walking (2026-10-16)
- **Problem (research b156 M1 / M2, b167):** a standing ped turned only by `StandTurnR180` (beyond 135 degrees) or
  pivoted on the spot at 45 deg/s; a walking ped stopped to pivot beyond 60 degrees. The `StandTurnR45..135` and
  `WalkTurnR45..180` clips were loaded but never played.
- **Retail [code + data] (b167; main checked the branch directions and the edge addresses of `82E2A500` in the recomp
  asm; the edge values were read from the image by the research agent):** the locomotion update `82E36870` ->
  `82E29540` -> `82E29D98` posts a turn intent through `82E2A500` while no turn or transition runs (loco +0x84 /
  +0x86). The angle (`82E2ABF8`) is the signed angle from the facing to the desired direction (`82E2AAA0`, b169,
  main-checked): a forced facing (queue +0x820 when +0x834 & 0x20), else the first face request at ped+0xB0 more than
  0.1 m away, else an override of the bot (mover slot 0x14), else NavPower's steering velocity normalised (mover slot
  0x0C, `82C47198`); zero steering posts no turn. The 4.25 that `82E2ABF8` passes on is not used by the angle (it is
  stored at cmd +0x80). Buckets: from
  25 / 70 / 115 / 160 degrees the 45 / 90 / 135 / 180 turn of that side (`0x8209994C` / `0x8218155C` / `0x822F929C` /
  `0x82099154` and their negatives); below 25 no turn. A post clears the steering (command +0x64 = 0); the motion
  graph state that receives it picks the clip (IdleDefault `StandTurnR*`, Walk `WalkTurnR*` in a branch window,
  `blendedturn` 0.15 / exit 0.1, left = the mirrored right clip).
- **Change:** `wander::turn_bucket` and `WanderParams::turn_edges` (data, mod-tunable); `NavOutput::turn_degrees` /
  `steer`; the step posts the bucket's turn from Idle / Stop / Walk with no steering; the anim player plays
  `names::stand_turn` / `names::walk_turn` (`Locomotion::WalkTurnLeft` / `WalkTurnRight` return to the walk cycle at
  the clip's exit); the heading comes from the clip only during a turn.
- **NOT RETAIL YET (ours):** the angle is to the next corner (our stand-in for NavPower's steering direction; while our
  avoidance steers no turn is posted); the forced facing and the face-request list are not ported; our steering below 25 degrees turns at 45 deg/s (NavPower's rate is not decoded: measure it in the recomp,
  b158 item 2); while our avoidance stand-in steers no turn is posted and the old stop / pivot rule runs; a set
  without the bucket's clip (a mod or test set) pivots by the steering instead; run turns (`ChaseRunTurnR*`) are not
  ported (a running ped still stops to pivot beyond 60 degrees); ForceStop, strafe turns (mask 0xCC) and the
  requester masks are not ported; the walk turn's return blend to the walk cycle (0.1) is ours.

## Peds that get pinned on a corner step away and choose a new path (2026-10-16)
- **Problem:** with the turn buckets, a DownTown data run (30 peds, 2 min round the ramp) had 31 walking-in-place
  episodes: a ped walking at a funnel corner on the mesh boundary it cannot step onto, with the next corner out of
  straight reach from where it stands; a re-plan gave the same path back. Also the path search could repeat a corner.
- **Change (ours; NavPower handles this internally and is not decoded):** consecutive duplicate corners are dropped;
  a walking ped inside the corner radius whose distance to the corner stops shrinking for 0.5 s (`stall_seconds`)
  moves on to the next corner; a walking ped that stays within 0.3 m (`stuck_distance`) for 2 s (`stuck_seconds`)
  gives up its target and plans the next one away from the old target with the long fan skipped. That reverse is
  retail's goal event 4 (`82E31498`: next target direction = old target to ped, pm3 / b158 item 9), whose sender in
  NavPower is not read; using it for a pinned ped is ours. All three values are `WanderParams` fields.
- **Verification:** `a_pinned_walking_ped_gives_up_and_plans_away_from_its_target`, `turn_bucket_edges_follow_retail`,
  `turn_buckets_pick_the_stand_and_walk_turn_clips` (skate-core peds 130 pass); data-gated
  `downtown_peds_at_the_ramp_never_walk_in_place` 0 episodes (31 before the fix), ped nav data 5 pass; skate-game
  living_world 93 pass. Play-tested by the user 2026-10-16 (hails-additions 064706e9, with the measured steer rate):
  "peds seem to turn much better".

## Zombies reach their goal within the mover's 2 m (2026-10-16)
- ZombieFollow's "goal reached" is the mover's vfunc +124 (`82E2BC70`, main read in the recomp asm): 3D distance
  within the arrival radius `+0x0C`, and every writer found sets it to `0x82060C50` 2.0 m (`82E35E80`, mover modes
  2 / 3 / 4 / 6; b158). Ours was 0.5 m flat [inferred]. Now 2.0 m in 3D, mod key `ped_brain.zombie_arrive_distance`;
  test `zombies_follow_the_player_and_mill_round_them` checks 2.1 m (not reached) and 1.92 m (reached).

## Ped steering rate measured in the recomp: 255 deg/s, and the ped_nav mod values (2026-10-16)
- **Problem:** our walking peds steered at 45 deg/s (a locomotion record value read as degrees per second, meaning
  unverified); NavPower's real rate was unknown (b158 item 2, b169).
- **Measurement [trace, recomp]:** a new recomp trace line `PEDFACE` (research hooks on the ped mover's think
  `82E2CB08` and the steering getter `82E2D180`; offsets from b170, main-checked: ped = mover +4, the turn flag byte
  `*(ped+0x1704)+0x84`) logs each ped's facing, position, turn / transition flags and NavPower's last steering
  direction about 3 times a second (the mover think's own rate). Scripted standing runs at PCU Library (user: "much
  better location") and DownTown Aletown: 30 peds, 12,087 lines, 0 malformed. A walking ped's facing is within 5 deg of
  the steering direction in nearly every sample (no facing lag). Outside turn clips the walking direction changes p50 0,
  p90 19, p95 48, p99 149, max 257 deg/s; turn clips p50 68, p90 178, max 328 deg/s.
- **Change:** `WanderParams::steer_rate` 255 deg/s for the walking steer (the 45 deg/s value stays for standing peds
  turning to face a point or a waypoint). Code candidate for the value (b158, meaning unverified): the bot value +0x80 =
  clamp(255 x max(1, s), 90, 720) of the truncated speed (`82E2D260`), 255 at s = 1; the 510 it would give at the
  wander speed 2.0 never shows in the trace, so the measured 255 is used.
- **Moddability:** new mod block `living_world.ped_nav {steer_rate, turn_edges, stall_seconds, stuck_distance,
  stuck_seconds, crosswalk}`, held in `LivingWorldSettings` (reset when the mod is disabled) and copied into the nav
  settings; before this no ped nav value (crosswalk rule included) was reachable from a mod.
- **Tools:** `tools/claude-skills/recomp-research/tools/ped_face_rate.py` (PEDFACE) and `ped_turn_rate.py` (PEDXYZ);
  scripts `.local/recomp/scripts/pedface_pcu_library.txt` / `pedface_stand.txt`. Credit: the skate3recomp static
  recompilation and its research hooks.
- **Verification:** `ped_nav_is_mod_reachable_and_reset_on_disable`; ped suites (see the commit). Not play-tested yet:
  walking peds now follow path corners quickly instead of arcing slowly.

## Ped normal maps: the RGB layout (2026-10-16)
- **Problem (user, verbatim: "the lighting of peds in the recomp are much better and smoother ours in the rust engine
  are rough and blocky feeling dark patches", "mainly on the shadowed side"; research b171, main-checked):** the
  living-world model exporter read every normal map as retail's two-channel layout (x in alpha, y in green). Ped normal
  maps are DXT1 (rx2 format 82): no alpha, so x came out 1 and z 0 for every texel. The installed `female_adult_1.glb`
  body and hair normal textures read R 218..255, B exactly 128 (main checked the pixels): every shaded normal lay flat
  along the surface tangent, which lights as blotches that change per UV island and DXT block, worst away from the sun.
- **Change:** `living_world_models._normal_png` picks the layout from the source: DXT5 two-channel (x in alpha;
  vehicles, unchanged) or DXT1 RGB (x in red; peds); the decoded PNG no longer carries the format, so a constant alpha
  marks the RGB layout (a mod texture of either kind converts too). Test `NormalMaps` in
  `tools/asset_pipeline/test_living_world.py` (synthetic, both layouts); the vehicle exporter tests still pass.
- **Needs** the setup `livingworld` group re-run to re-export the ped models. Play-tested by the user 2026-10-16 after the
  re-run (hails-additions 064706e9): "the shadows look much better". Open (b171): peds use StandardMaterial with
  the flat global ambient while the player's character material gets SH fill (retail's ped ambient not read); peds have
  no RenderLayers [0, 28] (no shadow on the baked world); the normal map X sign after the re-export.

## Moods: DisableAllMoods and prerequisite kind 8 (2026-10-17)

From the stuck-ped research for the laptop session 2026-10-17 (b175, b176 in `.local/research/`):
- `DisableAllMoods` now stops the mood producer. Retail's producer `sub_82E41B98` skips a ped while `brain+0xCCE` bit
  0x08 is set (set and restored by `826A5A20` / `826A5A50`) [code, b176]; ours set and restored `moods_disabled` but
  nothing read it. Now `peds.rs` skips `produce` for a busy ped or while it holds. No stock pedestrian graph uses
  DisableAllMoods, so stock behaviour does not change.
- Prerequisite kind 8 is the pending `flee` want (want slot 10, `brain+540` bit 0x80, `sub_82BFEA60`) [code, b176],
  `MoodContext::flee_pending`; before it was always false. Stock `greeted` uses it as "not fleeing" (default,
  default_female, security_street): a fleeing ped is no longer greeted.
- Checked and NOT changed: the return greet repeating from one `greeted` record for its 30 s lifetime is retail's
  behaviour too (no record consumption or hidden cooldown in the tick / expire / perception / producer chain while the
  9ABC gate is off, b176 [code]); a recomp trace counting the raises per greet would confirm it.
- Tests: `prerequisite_kind_8_reads_the_pending_flee_want` (skate-core mood tests).

## A fast hit on a ped comes from the bodies meeting (2026-10-17)

- **Problem:** user (laptop session game-20261017-143646): "while you can bump them, running into them fast should
  cause bails". 12 ped knockdowns at 3.5 to 7.9 m/s, no skater wipeout after any of them.
- **Retail [code] (research b185):** there is no ped-hit bail. The skater bails through the ordinary skeleton contact
  solve: the ped's body parts are group 11 in it (`sub_82BD4A30`), and the ground wipeout checks (`82D8F9E0`: contact
  force 8.5, arms 30.0, displacement 0.4, squash 0.6, x 0.3 mid-trick) read those contacts. The ped's reaction
  (`sub_82E38FB8` kind 5) only writes ped state.
- **Ours before:** `skater_hit` knocked the ped down by distance (ped radius + 0.35 m), about 0.35 m before the bodies
  met, and from the next tick the knocked-down ped's body left the solve (`ped_body::in_collision`), so the skater
  never touched it.
- **Change:** `skeleton_feedback` also collects the group-11 contacts (`SkaterRuntime::ped_hits`: proxy id, point,
  relative speed); for the local skater and a ped whose body is in its solve, `advance_peds` takes the reaction from
  those contacts. The distance test stays for other skaters and peds without a body (NOT RETAIL). NOT RETAIL YET:
  the closing speed is the relative speed at the contact (retail: the ped part's velocity after the solve).
- **Open:** whether retail keeps the ped's body during the knock-down (b145 trace pending); whether the skater meets
  body A (group 9).
- **Verification:** skate-game ped / vehicle / living_world tests 133 pass; muted DownTown run (below). Not
  play-tested: run into a ped fast. Muted DownTown run: no errors, peds spawn, the map switch commits.


## A ped that lost its prop no longer stalls a conversation (2026-10-17)

- **Problem:** user (laptop session game-20261017-143646): "I feel like im still seeing alot of peds just standing
  around and making small groups." Three conversations never finished (ids 60, 70, 85: 1 turn in 236 s, or stuck at
  state 3 until the log ended), and their members stayed put; 45% of the Plugin `PED_STILL` lines came from them.
- **Root cause [log + code] (research b186):** a ped whose seat or newspaper box went away without ExitPlugin (streamed
  out, disabled, moved) dropped `mind.prop` silently but kept the prop's waypoint and facing. As a later conversation
  partner, LockFirstWaypoint saw a held slot and did not lock the conversation's waypoint, so TurnToFace turned to the
  old seat facing while IsFacingWaypointOrientation tested the direction to the conversation centre. The graph flipped
  Speak / TurnToFace every 2 to 3 ticks, every Speak Begin restarted the 3 s turn timer, and the turn never passed.
- **Retail [code]:** LockFirstWaypoint (`826A0580`) only locks when the waypoint slot `ped+0x1718` is empty (our gate
  is retail). The slot belongs to the plugin and leaving the plugin releases it; the stale value was ours.
- **Change:** `PedBrain::release_plugin_waypoint` (skate-core) clears the waypoint and its facing; every plugin exit
  uses it: the prop ExitPlugin, the conversation ExitPlugin (which kept the facing before) and the silent prop drop,
  which now also releases the prop's waypoint and logs `PED_PLUGIN_PROP ped=#N leaves <class> <id> (dropped)`.
  Pick odds, turn length and the gather timer are unchanged (they match retail and the logs).
- **Tests:** `a_released_prop_waypoint_lets_a_conversation_lock_its_own` (skate-core brain tests); the 5-turn run stays
  covered by `a_conversation_gathers_starts_and_runs_five_turns`. skate-core peds tests 132 pass; skate-game checks.
- **Not play-tested:** stand still in DownTown near benches for a few minutes; conversations should finish in 20 to
  40 s and `(dropped)` leaves show where props went away.
- **Open:** the retail writer that clears `ped+0x1718` on plugin end is not decoded; whether retail refuses a partner
  that is in NoRoad; the `start / leave result=empty` churn of one ped every 2 ticks after its conversation emptied
  (cause unverified, may go away with this fix).

## A full mood store evicts by priority (2026-10-17)

- **Ours before:** when a ped's 10 mood records were full, the oldest record went, so a long-lived collision or
  being-chased record could be pushed out by a stream of presence events.
- **Retail [code] (research b189; main re-read the compares in the recomp):** `sub_82E414D8` (only caller
  `sub_82E41060`, after no record matched the same instigator, second entity and category) drops the new record when
  the store is full and it has both an instigator and a second entity; otherwise `sub_82E441C8` scans the records in
  slot order from (none, 0, -1.0) and takes the one whose category Priority is unsigned greater, or equal with a
  strictly greater age. So the numerically highest Priority goes, the oldest on a tie, the first in slot order on an
  exact tie; the new record's own Priority is never compared. Priority is the category field
  `Hash_1A3FB63DFDAE0C4F` read through the mood manager's vfunc +32 (`82BFF308`); stock values 1 (collision,
  beingchased, nearbysession: kept longest) to 6 (litter: evicted first).
- **Change:** `MoodCategory::priority` (setup data, absent = 0), `MoodStore::post_ranked` (the scan above); the ped
  system posts with the categories' priorities. `post` keeps an all-equal priority for tests.
- **Tests:** `a_full_store_evicts_by_priority_then_age` (b189's case: the youngest litter record goes before an old
  collision; of two equal-age records the first goes); skate-core living_world 343 pass, skate-game ped tests 28 pass.
  Not play-tested.
- **Open:** the attribute default at `0x830D0850` read as a u32 (taken as 0; every stock category has a Priority).

## Peds stuck at walls: retail has no give-up (research, 2026-10-17)

- **Retail [code] (research b190, not traced):** the ambient ped's NavPower bot (`82E974A8`) snaps to the mesh,
  re-requests its path when it turns invalid (`82E95FD0` -> `82E957F8`) and picks a new target only when the bot is
  done. It keeps a no-progress timer (`82E965D0`, bot +0x21C, limit min(settings +8, speed x 2.0 x 0.0625)) that no
  code was found to read. `82E2AAA0` / `82E2ABF8`, named as stuck candidates in b175, are the desired-direction and
  turn helpers.
- **Ours:** the stuck watch in `wander.rs` (0.3 m in 2.0 s, then give up and plan away) stays, now labelled NOT RETAIL
  in the code (it was credited to retail's event 4). A later port: the retail timer as the measurement, the give-up
  as a mod-switchable non-retail value.

## Peds stay solid while they react (2026-10-17)

- **Problem:** user (laptop recomp session 175036 notes): "peds are much more solid in retail than ours. you cannot
  move through a ped."
- **Root cause (research b203, `.local/research/b203-peds-solid.md`; main checked `ped_body.rs`):** our
  `push_ped_proxies` took a ped's bodies out of the skater solve for the whole stumble or knock-down (the
  `in_collision` filter). Every touch starts at least a stumble (retail gate 0), so after the first touch the skater
  rolled through the ped.
- **Retail:** [trace] every ped body removal in the laptop session comes from the ped collision object's destructor
  (`sub_82E22D50`, call at `82E22D90`, ped despawn); the "remove both bodies" slot `sub_82E0D5F0` never runs. [code]
  `sub_82E268C8` builds body A (group 9) and body B (group 11). So the bodies stay in the world for the ped's whole
  life, through every reaction.
- **Change:** `ped_body::push_ped_proxies` adds every near ped's bodies, reacting or not. New hits during a running
  reaction are still skipped (`peds.rs`, retail `sub_82E38FB8` controller byte +9), so the ped is not knocked down
  again every tick; the skater is now pushed back by the bodies the whole time.
- **Verification:** skate-game ped / ped body tests pass (48). Not play-tested.
- **Open (b203):** whether the skater also pairs with body A (group 9); whether retail treats the re-posed ped part
  as keyframed or finite mass; the reaction input is the relative contact speed, not the ped part's speed after the
  solve (NOT RETAIL YET); the proxy has zero inverse inertia. Tune nothing (no mass changes) before those are read.

## Hand props: retail rotation order (2026-10-17)

- **Problem:** user: "ped hand props are NOT held properly with correct positioning/rotation and attachpoints".
- **Retail [code] (research b202, b204, b205):** the per-frame placement `82E3E4F0` (only while the prop is in hand)
  writes World = H * T(offset) * R * inverse(P) through `82E3E1B0`: H = the RightHandProp bone times the ped's world
  matrix (`82E218A0`, slot 3 of the bone table filled by name in `82E33070`); R = Rz * Ry * Rx from the record's
  degrees (`82473A08` sin / cos per axis; first basis row [cy cz, cy sz, -sy]); P = the prop template's attach
  locator (`template+0x78`, the type-0 entry of the part's EB001B point list, written by `82C4E128`).
- **Ours before:** the same bone (`HAND_PROP_BONE`) and T * R, but R in XYZ order and no inverse(P).
- **Change:** `HandPropRecord::local` uses `EulerRot::ZYX` (z, y, x). Test `record_rotation_is_rz_ry_rx_like_retail`
  (the stock cellphone's 15 / -10 / 15 degrees against Rz * Ry * Rx).
- **Attach locator (same day):** setup now exports P. `dynamic_props.template_attach` reads the part's EB001B point
  list (count +4) and its EB001E section (0x70 B points: matrix +0x00, type +0x60) and keeps the type-0 point;
  `hand_props.json` carries it as `attach` (row vectors), and `HandPropRecord::local` is T * R * inverse(attach), so
  the model's grip point, not its origin, lands at the record's offset. [data] All 22 hand props with a template have
  one rigid type-0 point, and every one's tInstance and model matrices are identity, so P is already in the glb's
  space (e.g. the bags', briefcase's and duffle bag's points sit 0.34 m up at the handle, most turned about 90
  degrees). Identity when absent (a mod model, an older install). `hand_props.py` joined the maps group's setup
  fingerprint (it runs there): release copies ask for Refresh Game Files once. Test
  `attach_locator_lands_on_the_offset`. Not play-tested.
- **Modding:** a mod model without a locator is held by its origin (identity attach); rotation and offset stay the
  record's (tables) values. Open: an `attach` override per record for mods.

## Peds standing in groups: mostly the stalled conversation, not the rate (research, 2026-10-17)

- **Report:** laptop session game-20261017-143646 (build e44f93a): peds "standing around and making small groups";
  PED_STILL Plugin 633 of 856 samples, conversation 530.
- **Finding [log] (research b211, `.local/research/b211-ped-conversation-rates.md`):** that build lacks 01383e32 (main
  checked with `git merge-base`). Three conversations (60, 70, 85) give 245 of the 530 samples: each stuck at state 3,
  the speaker flipping TurnToFaceWaypointOrientation / ListenToSpeaker every tick, at least two of them peds that had
  lost their seat but kept its waypoint, the case 01383e32 fixed. Three more waited out the 30 s gather and the 60 s
  PluginTimeout. Together about 57 % of the samples.
- **Rules:** no difference from retail found that starts more conversations or keeps them longer (start from the
  converse mood rows, 50 m exclusion, 30 s gather, 5 turns of 3.0 s, 60 s PluginTimeout). One difference goes the
  other way: ours never adds a third member (retail passers-by can join); research b212 on that and on despawned
  members that never leave.
- **Next:** confirm in a new laptop session that the flip and the 60 s stalls are gone before changing any rate.

## Passers-by join conversations; the founders pass the same test (2026-10-17)

- **Problem:** ours never added a third member to a conversation, and the starter and its target joined unconditionally
  (b211, above). Research b212 (`.local/research/b212-conversation-join-despawn.md`); main re-read every address below.
- **Retail [code]:** a conversation area is an ordinary offer (vtable `0x8232B798`, add / remove in slots 15 / 16), so
  passers-by reach it through the same wandering-offer scan as seats and ATMs (`826C0058` -> `826BFE18`): no current
  plugin, not refused, `|dy| < 1.8`, horizontal distance under the offer radius (class field `E2101F2B17A0E6B5`,
  `waypoint_conversation` 20.0 [data]), the descriptor's transfer expression, the `plugin_odds` roll, then attach.
  The transfer reads `ConversationIsGatheringMembers` (`8269CE40` -> the conversation's vf4: room and state 0 / 1) and
  `EntityAllowedToJoinConversation` (`8269CD70` -> vf8: the ped's type fills a free row slot). The add (`82E1D380`)
  turns a ped away unless vf4 holds and the slot finder (`82E1DB50`) succeeds; `82E1DC70` builds the candidate rows on
  the first join and on every later join keeps only the rows the joiner matched. The spawner (`826A6CD8`) adds the
  starter and then the target through the same manager call (slot 13 = `826BFE18`) and ignores the result.
- **Change:** `Conversation::join` takes the ped's entity type and the rows, refuses a misfit and narrows the candidate
  rows (the start then picks only among rows every member fits); the member cap comes from the descriptor
  (`maxNumberOfParticipants`, 3). `plugins::offer_conversation` runs the offer test for one ped; the 11-tick scan offers
  every gathering area after the placed props ([inference]: registration order not read), and SpawnConversationArea
  puts the starter and the target (a ped only) through it. The target's refusal memory is written by its own think
  (one tick later at most). A ped in a conversation re-stores the area's refusal each tick like a prop (`8269F248`).
  New log line `PED_CONVERSATION id=.. join ped=.. members=.. state=..`; the start line gains `joined=<starter>/<target>`.
- **Moddable:** offer radius (class field), member cap and transfer conditions (descriptor XML, by name), odds
  (`plugin_odds` `waypoint_conversation`), rows and categories (tables); the conditions are named so a mod descriptor
  can combine them.
- **Tests:** skate-core `a_third_member_joins_while_gathering_and_narrows_the_rows`, `nobody_joins_after_the_start`,
  `a_passer_by_joins_a_gathering_conversation_through_the_offer`; the stock-data test checks radius 20, the transfer
  names and the odds key.
- **Open:** retail builds candidates from every category of the area's group, ours from one category picked by
  probability (same for stock data: one category); retail keeps a leaver's slot key; an area whose founders were both
  refused is removed at once here (retail behaviour not read).

## On foot: peds give when run into, benches are walked over, a pushed seat throws its ped off (2026-10-17)

- **Report:** laptop session game-20261017-195628 (build 17b3d0d), the user's words: "Peds when run into are too solid.
  They should be a little easier to push when run into."; "Running into a bench caused me to go through it instead of
  walking over it. In retail I walked over the same bench at running speeds."; "Pushing a bench in retail bumps peds
  physically and doesnt unseat them, it knocks them off the seat".
- **Peds too solid (research b217, main checked the chain):** on foot the walker's last-mile capsule sweep
  (`physics/solid_contacts.rs` `constrained_position`) stops at the ped body proxies (they are in
  `network_proxies.solids`, `ped_body.rs` `push_ped_proxies`) as hard walls, so the skeleton never touched a ped,
  `ped_hits` stayed empty and the ped never reacted. Retail [code, b144 / b146 / b147 / b217]: the ped body is
  keyframed from the skeleton with a finite mass; the contact only decides stumble or knock-down, and the "give" is the
  stumble clip's root motion [trace: a stumbled ped moved 0.24-1.1 m in about 1 s in recomp-all-20261017-175036].
  **Change:** the sweep records the solids that stopped it; ped parts come back with the walker's closing speed and the
  ped side turns that into the part's speed after the hit by momentum (`ped_body.walker_mass`, mod-facing, 75 kg, NOT
  RETAIL YET: retail's on-foot path into peds is not found), which feeds the existing stumble / knock-down rules. Log
  `PED_BODY_CONTACT ped part by=walker closing part_speed`. Test `a_walker_stopped_by_a_ped_reports_the_part`.
- **Bench walked through (research b218, main checked the probe call sites):** every on-foot query saw only the map
  world, so a prop was never step geometry; the skeleton's parts were dragged into the bench and shoved it. Retail
  [code]: the walker's query batches use pool mask 7; pool bit 2 (`82764AB0`: world+0x5580) is the pool the grab
  request asks with mask 2 (`8275FD00`), the DMOs [inference]; nearest hit across pools, the earlier pool on ties
  (`8276EE88`). **Change:** `offboard::BipedWorld` (the map plus the prop world, derefs to the map) and
  `GamePhysics::biped_world()`; the contact toolkit scene, the ground geometry query and the biped air selector answer
  pool 2 from the prop world. The prop the player holds is left out (NOT RETAIL YET: retail's held-DMO filter is not
  decoded). Board states are unchanged. Test `biped_lines_see_the_prop_world_as_pool_two_without_the_held_prop`.
- **Seated ped on a pushed bench (research b219, main re-read `82E3DBE0`):** retail never knocks a seated ped down with
  its own seat (`sub_82E38400` gives a plugin object kind 3 or 0, never 5). The owned object reports each move as a
  step vector (`82E3DBE0`): |d|^2 <= 2.5e-5 is added to `brain+3168` and carried by the root update; a bigger step, with
  OwnPluginObject and the byte `[ped+5888]+1416` == 0, queues Avoid / Jump / Forward (SitAvoidFwd). Ours read the
  gate as a speed (it fired at 5 mm/s) and raised the jump for one tick only, so it raced the plugin's stand-up (log:
  #141 avoid and "Sit ended mid-clip, standing up" in the same tick, seated again two ticks later). **Change:** the
  seat's move over one step of `SEAT_PUSH_STEP_SECONDS` is compared (1/60 s: the DMO post-update `82C562D0` sends
  its pose change once per sim tick, research b220, so the gate is 0.3 m/s); the jump is held until its clip starts. Logs `PED_SIT_AVOID ... locked=`, `played=`,
  `PED_SIT_AVOID_DONE moved=`.
- **Which props the walker steps on (research b221-b223, recomp trace, main re-read the type setters):** retail's DMO
  pool rebuilds its query meshes every frame with rejection flags `1 << type` (`827778C0`); the walker's contact
  probes ask with mask 0x6000, so types 13 and 14 are never its ground. [trace, `.local/research/prop-types/run1`,
  hooks PROPTYPE / PROPSETTYPE on the recomp fork] every DMO spawn (`82C4E940` -> `82E0E338`) gets type 12 or 14; a
  ped's hand prop is made type 14 and switches to 13 the moment the ped takes it (slot 32 `82C56B00`), back to 14 on
  release (slot 33 `82C56BA0`: 12 when the type's mass is at least 5.5 kg, `0x822F8C68`, else 14, plus a 30 s timer).
  [code b223] the compared value is the type's mass (`Hash_E5778CDD4576D890`, record +304). So the 50 kg flat bench is
  ground for the walker, the 1.75 kg cone, cardboard box, trash lid and garbage bag are not. **Change:** the prop layer
  keeps one set of query meshes per prop; each step gives them the type flag from the template mass (mod-facing
  `props.solver.walkable_mass`, 5.5 kg) or 13 while the player holds the prop (NOT RETAIL YET: the player's grab
  switching to 13 is not traced; the ped's pickup is); this replaces the earlier skip of the held prop's triangles.
  The ground geometry query skips walker types on prop meshes too (NOT RETAIL YET: that query's own mask is not read).
  Test `props_get_retails_collision_types_by_mass`.
- **The jump's gate byte (research b220):** `[ped+5888]+1416` is the ped's intent slot #3 "queued" flag (set on
  enqueue `82E29760`, cleared when the intent ends `82E29B30`): 0 for a seated ped, 1 while the jump plays, so more
  seat steps queue no second jump, the same outcome as holding the jump until its clip starts.
- **Open:** whether the jump clip's root motion takes the ped off the seat (the next session's `PED_SIT_AVOID_DONE
  moved=` shows it); the small-step carry is not ported (the moved seat already re-places its ped); the walker mass; per-template opt-out of props as biped ground.

## On foot: ped contacts were never reported; walker step log (2026-10-18)

- **Report:** laptop session game-20261018-120611: peds still too solid on foot, and walking over the bench still bumps
  sometimes. The log had 0 `PED_BODY_CONTACT ... by=walker` lines; all 16 `PED_SKATER_CONTACT` lines came from NPC
  skaters.
- **Root cause (research b223, main checked every line):** each ped part proxy id was `PED_PROXY_TAG | ped id << 2 |
  part` with the ped id from `LivingWorldId::to_u64` (kind 1 in bit 56). Shifted by 2 the kind lands in bit 58, so the
  id's top 16 bits read 0x5445, not the tag 0x5045, and both tag tests (`solid_contacts.rs` walker hits,
  `skeleton_feedback.rs` board hits) dropped every real ped part. On foot the parts still stopped the walker's capsule
  (the "too solid" feel) but no ped ever got the contact; on board the parts were filed as vehicle hits. The tests
  used a bare id 7, which has no kind bits. So the walker part of the 2026-10-17 section above never worked in a build.
- **Change:** the proxy id carries the ped's serial (`ped_body::proxy_id`, every proxy is a ped), and one decoder
  (`ped_body::proxy_serial`) serves both tag tests and the ped's own match (`peds.rs`). Ids stay stable per ped (a
  serial never repeats within its kind), so they remain the multiplayer authority keys. Test
  `a_part_is_a_capsule_between_its_bones_in_group_11` now covers the largest serial and an id with a kind byte.
- **Walker step log:** `WALKER_STEP prop=<id> (<template>) <outcome> rise <m> edge_rise <m> ahead <m> speed <m/s>
  flags <packet flags> kind <slope kind> at [x, y, z]` when the walker's contact target or edge is on a prop, written
  when the prop or the outcome changes (`WALKER_STEP prop=<id> left` when no prop is involved any more). Outcomes: `step`
  (target on the prop, rise over the 5 cm step threshold of the contact classifier `82D837C8`), `flat`, `no-candidate`
  (packet flag 1024), `obstacle` (contact correction active), `no-target`. Diagnostics only, no behaviour change.
  The prop is the nearest streamed-in prop box within 0.1 m of the target or edge (`PropDynamics::nearest_body`).
- **Open:** what the next bench bump logs (`WALKER_STEP` next to `PROP_SHOVE`); with the hits now reported, whether peds
  give enough on foot (`ped_body.walker_mass`, NOT RETAIL YET).

## The tazer draws, cycles and holsters with its clips and the taser in hand (2026-10-18)

- **Problem:** user (laptop session 2026-10-18 152526): "I also pissed off a lady ped and it tased me. However, the
  animation was not in yet." The taze logic worked (PED_TAZE -> SKATER_TAKEDOWN), but the ped played no clip and held
  no taser.
- **Retail (research b248, b153; main checked our anchors):** [data] TazerInto = `SG_TAZR_STND2TAZR_F_0_N`, TazerCyc
  = `SG_TAZR_TAZR_F_0_CYC`, TazerOut = `SG_TAZR_TAZR2STND_F_0_N`, all on the upper-body channel (ped vfn248 `82E3A3F0`
  -> vfn240 with blends 0.2) [code]. DrawTazer (`826A8258`) plays TazerInto; once timer 37 (0.133 s) runs out the
  "tazr" hand prop (tasergun_lw) is requested if the hand was empty at the draw. The ped update `82E3ED50` starts
  TazerCyc and sets the drawn state 2 when TazerInto has 0.2 s left (HasDrawnTazer tests state 2, b153 M1; ours
  used 0 and drew on the timer alone). EndTaze (`826A85A8`) plays TazerOut and sets state 3; the draw request,
  state 0 and the prop's deletion wait for TazerOut's last 0.2 s (b153 M2; ours cleared at once). The player's side
  is a plain takedown wipeout in retail too (no tased clip found; the only taze extra is a "ZAP!" / "Tazed!" HUD
  line, not ported).
- **Change:** brain `DrawTazer` / `EndTaze` post `ChaseRequest::TazerClip`; the body loop plays it on the channel
  (`PED_TAZER_CLIP`) and records whether the anim set had it; `sync_hand_props` calls
  `PedBrain::update_tazer_clip` (drawn / put away by the clip's time left; a clip the set lacks counts as ended, so
  no ped gets stuck). Settings `tazer_clip_blend` / `tazer_clip_end` (0.2 / 0.2); clips and prop through the
  entity animation and hand prop tables (mods).
- **Tests:** `a_tazer_is_drawn_then_hits_its_target_once` (draw clip, prop at timer 37, drawn at 0.2 s left),
  `putting_the_tazer_away_waits_for_the_holster_clip_then_deletes_the_prop` (also: no clip in the set, a hand that
  already holds a prop). Not play-tested.
- **Open:** whether our channel loops a `_CYC` clip (one-shots fade at their end; the carry pose then holds the
  taser), the hand bone (we use RIGHTHANDPROP), the HUD line.

## On foot: the player passes a ped; the ped reacts to its part's speed after the solve (2026-10-18)

- **Problem:** since 2026-10-17 the on-foot walker's capsule stopped at every ped body part, so walking into a ped felt
  like hitting a wall, and the ped reacted to the walker's closing speed scaled by a made-up 75 kg walker mass
  (`ped_body.walker_mass`, NOT RETAIL) instead of anything the solve computed.
- **Retail:** the walker's queries never include the PedestrianPool (b266 [code]: builders `82764AB0` / BE8 / CF8 use
  only world+0x5580 / +0x5584); only the skater's skeleton meets the ped parts, in the Skater x Ped pair job
  (`8277AB70` / `8277A508`, contacts only) and the skeleton solve. The ped's reaction `sub_82E38FB8` kind 5 reads the
  part's own linear / angular speed after that solve: knock-down above 3.0, otherwise the standing stumble (b272
  [code]); the ped moves through that reaction, there is no root write-back. Two laptop recomp sessions
  (recomp-pedbody-20261018-205249 / -210429, 34 player x ped contacts, 9 of them on foot; analysis
  `.local/research/b271-pedbody-session.md`) [trace]: on foot the player keeps moving through the contact at up to
  ~6 m/s and the on-foot wipeout check `82D90148` never requests a bail (body region strength at most 3.95 of 20.0).
  On board the user: "there were several bails caused by contact with peds" (board check `82D8F9E0`, unchanged).
- **Change:** `solid_contacts::constrained_position` skips group-11 solids (the walker block and its hit list
  `walker_ped_hits` are gone, as is `ped_body.walker_mass`); the ped parts (already 10 / 20 / 20 kg) are observed in
  the shared solve (`Proxies::observed`, never written back) and `capture_dynamics_reactions` records each part's
  velocity after the solve (`Proxies::ped_after`: before + the solver's velocity change);
  `skeleton_feedback::publish` gives every ped hit that part's |v|, which `peds.rs` feeds to the retail
  `skater_reaction` rule. Log `PED_BODY_CONTACT ped part part_speed`.
- **Moddability / multiplayer:** part shapes, masses, bones, range and look-ahead stay in `living_world.ped_body`
  (mod patch `ped_body {enabled, parts, look_ahead, range}`; `walker_mass` removed); the 3.0 gates are the
  collision rules' values. The skater's owner computes the part speed in its own solve; the hit is keyed by the
  stable ped proxy id (`ped_body::proxy_id`); the ped's authority applies the reaction.
- **Verification:** tests `the_walker_passes_a_ped_part_but_not_a_mod_solid`,
  `an_observed_ped_part_reports_its_velocity_after_the_solve`; skate-mods 106, skate-game 803 pass. Not play-tested.
- **Part inertia (b275):** each part now turns in the solve with retail's capsule inertia (`ped_body::inverse_inertia`:
  `sub_82AE7770` / `sub_82AE7228`, R = max(r, 0.05 h), along the axis 0.4 R^2, across (1.6 R^3 + 0.75 h R^2 + 4 h^2 R
  + h^3) / (4 R + 3 h), times the mass; inverse torso 0.520984 / 6.25, forearms 2.037273 / 19.53125, matching the
  PEDPART trace [code + trace]), computed from the part's shape so a mod's part keeps a matching inertia. The hit
  carries the part's (|v|, |w|) after the solve into `skater_reaction`'s 3.0 / 3.0 gate; the log adds `part_spin`.
  Test `the_parts_turn_with_retails_capsule_inertia`; skate-game 804 pass.
- **Open:** whether our skeleton's pose error stays under the on-foot 0.3 m bail limit while passing a ped (retail
  never bailed on foot in the sessions); the ped dodge before contact (b273 / b274: retail's SkaterAvoidance
  closest-approach test, not ported yet).

## Peds fell over too easily at slow speeds: the reaction reads the torso only (2026-10-18)

- **Problem:** laptop session 2026-10-18 22:01 (trace-all, notes `game-20261018-220128.notes.log`), the user: "much
  better but they are a tiny bit too easy to knock over at slow speeds" (on the board and on foot). Six knock-downs
  at part speeds of 0.65 to 2.59 m/s.
- **Root cause:** the 3.0 m/s linear gate was never crossed in those knock-downs; the angular gate (|w| > 3.0 rad/s)
  was. Their `PED_BODY_CONTACT` lines show part_spin 6.4 to 11.4 (the three slow contacts without a knock-down: 1.4
  to 2.9). `peds.rs` took the largest |v| and the largest |w| over every part the skeleton touched, the forearms
  included. Two of the six were forearm-only touches, and a forearm's small inertia spins it fast from a light push.
- **Retail [code]:** `sub_82E38FB8` contact kind 5 reads one body only: `lwz r4,8(r9)` (the parts array) then
  `lwz r11,76(r4)` with no index, part 0, the 10 kg torso capsule (`.local/research/peds/fn_82E38FB8.txt` lines
  442 / 450, main-read). From it: |linear velocity| (`+32`) and |angular velocity| (`+48`), against the entity
  entry values (0 for stock entities) and then the set's 3.0 / 3.0 knock-down gate. No max or sum over parts.
- **Change:** any touched part still starts the reaction, but the speed that decides it is the touched ped's
  reaction part after the solve (`ped_body::reaction_part_speed`, `SkaterRuntime::ped_after`, which holds every
  observed part, touched or not). New setting `ped_body.reaction_part` (default 0, retail); without that part in
  the solve, the fastest touched part decides as before (ours). New log line `PED_REACTION_BODY ped part speed spin
  touched_max_speed touched_max_spin`, so a session shows both readings.
- **Moddability / multiplayer:** mod patch `ped_body {.., reaction_part}` (0..7, part index into `parts`). The
  choice is by part index, not by hit order, so every peer reads the same part.
- **Verification:** test `the_reaction_reads_the_torso_whichever_part_was_touched`. Research
  `.local/research/b283-ped-knockdown-gate.md`. Not play-tested.
- **Torso spin (research `.local/research/b288-ped-torso-spin.md`; main checked the friction literal and the
  combine):** a normal impulse on the capsule points through its axis, so one contact gives at most |w| / |v| =
  m h / I = 3.0 [arithmetic] (b283 said 4.0). The four torso knock-downs logged 4.8 to 5.5: friction at the capsule
  surface (radius 0.2) spins the torso about its long axis (inverse inertia 6.25), 4.4 to 6.3 for one contact with
  our part friction 0.5 (no retail source) and the max-combine (`physics/contact.rs:107`).
- **Retail part material [code, research `.local/research/b289-ped-part-friction.md`]:** every ped part body gets
  static / dynamic friction and restitution 0.0: `82E268C8` fills its three material slots (+80 / +84 / +88) from
  `0x82165A10` (main checked the `lis r11,-32234` / `lfs f0,23056(r11)` load and the three stores; the 0.0 value and
  the empty owner vfuncs +120 / +124 rest on the agent's dump), `82E0E4B0` writes them to the part definition +0x80
  and `82DC4588` copies them into the collision object, which the retail combine (`82763078`, ours
  `combine_contact_materials`) pairs with the skater bone's material (riding: friction 0). So a riding skater's
  contact with a ped part has no friction, and one contact can spin the torso at most 3.0 x |v| (under the gate at
  slow speeds).
- **Change:** the part friction is per part data (`ped_body.parts[].friction`, mod-reachable), retail default 0.0
  (`DEFAULT_PART_FRICTION`). Tests `ped_body_is_mod_reachable_and_reset_on_disable` (the new keys) and
  `a_part_is_a_capsule_between_its_bones_in_group_11` (friction 0).
- **Open:** the next session's `PED_REACTION_BODY` lines show whether slow knock-downs remain (b289 did not prove
  nothing rewrites the material after the part is built). The entity entry values are still the literal stock 0, not read from data. Whether retail's callback reads the
  body before or after the solve was not re-read in b283.

## Hand props drawn as nothing after a refresh: the asset check asks again (2026-10-18)

- **Problem:** laptop session 2026-10-18 22:01 notes: "ped water bottles have no model" (`PED_HAND_PROP ... waterbottle not
  created (no model)` for several peds). The refreshed game's log (`game-refresh-1792382523.stderr.log`) says
  `PED_HAND_PROPS 23 records, 0 with models`: no hand prop had a model on the laptop, not only the bottle. This
  PC's install has all 23 (`waterbottle.glb`, 208 vertices), so the export itself works.
- **Root cause [code + log]:** `asset_check::hand_props` only flagged a `hand_props.json` whose props all lack `attach`;
  a missing or unreadable file, or one where no prop has a `file`, gave no finding, so nothing offered the refresh
  that writes it. Why the laptop's install lost or never got the file is not in its logs (setup's own progress is not
  in the refresh log).
- **Change:** with the living world tables installed (`tables.json`), a missing / unreadable `hand_props.json` or one
  with no prop model is now the maps finding `hand prop models missing`, which offers the refresh of the maps group.
- **Verification:** test `hand_props_without_grip_points_are_stale` extended (no tables, no file, no model, no
  attach, current). Not run on the laptop.
- **Open:** whether that refresh then writes the file on the laptop; its next `PED_HAND_PROPS` line shows it.

## A bench pushed into water stops offering its seats (2026-10-18)

- **Problem:** laptop session 2026-10-18 22:01: benches pushed into water had no ped paths to them and kept their
  seats on offer. Asked whether a seat should stop being offered once its bench is in the water, the user: yes.
- **Findings (research `.local/research/b286-bench-in-water.md`, main-checked):** the offer gate already skips a
  `disabled` prop (`peds.rs` offer scan) and the drop path releases a ped walking to or sitting on one; the move check
  only ran for props with check limits. A seat under water is off the navmesh, so `PedNav::plan` finds no polygon
  and no path is planned: expected, not a bug (retail has no plugin reachability check either, only the 60 s
  PluginTimeout). Props have no water physics: a pushed bench sinks.
- **Change (NOT RETAIL, ours):** for a moved prop, `plugins::seats_in_water` asks `BoardWorld::water_surface_at` for
  each seat (`water_above` 0.05, `water_max_depth` 10.0 m); a seat in water switches the prop off like a tipped one
  (`PED_PLUGIN_PROP <class> <id> disabled (a seat in water), occupants evicted`), and it is back on once dry. The
  query runs only when a seat moved (cache `PedPluginProps::in_water`), so a resting prop costs nothing. The move
  check no longer depends on the type having check limits for this rule.
- **Moddability / multiplayer:** `PluginSettings` is now reachable through `living_world.ped_brain` (copied into the
  plugin props every think): `water_disables_seats` (default true), `water_max_depth` (0..=100). The host decides
  (`PluginProp` is host-owned), by stable prop id.
- **Verification:** tests `a_seat_in_water_switches_the_bench_off` (skate-core) and the `ped_brain` patch checks
  (skate-mods). Not play-tested.
- **First play (laptop 2026-10-19 00:19, build 0e2bb7d):** the user: "I pushed a bunch of benches in the water but I
  never saw a ped try to path to them." The log had no line about the rule, so it could not show whether the seats
  counted as wet (the benches may stand in shallow water with dry seats, or not be seat props). Added
  `PED_PLUGIN_WATER <class> <id> wet lowest_seat surface occupants` on every change of a moved seat prop's water
  state (the first check after a move always logs), with any water surface up to 2 m above the lowest seat.
- **Second play (laptop 2026-10-19 01:10, build edec93c):** two benches pushed into the Aletown canal; `PED_PLUGIN_WATER`
  logged only at the grabs (wet=false, surface=None) and never wet=true. Bench 1015 came to rest at
  (-162.84, 9.91, 450.60), its shape spanning y 8.93 to 10.89, so whether its seats were under the surface, above it
  (legs in shallow water), or where no water triangle was found is not in the log. Added `PED_PLUGIN_WATER_REST` once
  per stop of a moved seat prop: its water state, the lowest seat, and the nearest water surface within 5 m above or
  below it, next to the rule's own window (`water_above`, `water_max_depth`). The next session decides the fix.
- **The logs decide (2026-10-25, USER-ASKS 334):** every `PED_PLUGIN_WATER_REST` line of the sessions since
  2026-10-19 (12 session logs, laptop and desktop) read:
  - Aletown canal (DownTown map; laptop 2026-10-19 18:04, benches C8 / C9 / CA8888659E2DCE6F): at rest the lowest seats at y 9.62
    to 9.70, the canal surface at 8.934 [log]: legs in the water, seats 0.69 to 0.77 m over it, so `water_above` 0.05
    never counted them (wet=false on every line). These are the user's "pushed into the water" benches.
  - Laptop 2026-10-20 10:13 (benches E8 / EA / EBF5360F38E5D567): seats at y 0.25 to 0.44 against a surface at 0.353:
    wet=true at rest, flipping to false and back while the bench rocked (seats 0.04 to 0.09 over the surface).
  - Every other rest line: no water surface within 5 m (dry ground, a newspaper box, park benches).
  - The rest line itself repeated every few ticks while a prop jittered at rest (up to 9 lines per stop).
- **Change (NOT RETAIL, ours):** `PluginSettings::water_above` 0.05 -> 0.8 m, so a bench standing in water whose seat
  is up to 0.8 m over the surface counts as in the water (the canal benches; the rocking ones no longer flip); 1 m
  up on a ledge over the water stays dry. `water_above` is now in the mod patch next to `water_disables_seats` /
  `water_max_depth` (`living_world.ped_brain`, 0..=100). `PED_PLUGIN_WATER_REST` logs once per change of the rest
  state (wet, lowest seat in 10 cm steps) instead of every jitter.
- **Verification:** `a_bench_standing_in_a_canal_counts_as_wet` (the canal numbers: seat 9.62 wet, 9.95 dry),
  `a_seat_in_water_switches_the_bench_off`, the skate-mods patch checks. Play check: push a bench into the Aletown
  canal; its seats stop being offered (`PED_PLUGIN_PROP ... disabled (a seat in water)` unless it was already off as
  tipped).
- **Open:** a seated ped's exit through the drop path uses the same path as the tipped bench (not re-checked); the
  other PluginSettings values are not in the mod patch yet.

## A seated ped whose seat is dropped stands up (2026-10-19)

- **Problem (laptop session 2026-10-19 01:10, build edec93c):** the user: "I pushed a bench in the water but for some
  reason the ped sitting on it was left behind floating seated and unable to be bumped. This happened as the first
  bench I grabbed and pushed in the water." Log: ped #105 sat on bench 1015, then `PED_PLUGIN_PROP ... leaves
  waypoint_sit ... (dropped)` (before the grab, among `DMO_STREAM` evictions), its brain went on to a greet, but every
  later `PED_STILL` kept the position and the clip `NPC_SIT_IDLE03_N_0_CYC`, and bumps logged
  `PED_BODY_CONTACT_NO_REACTION ignore_standing=true`. A ped on a second bench jumped off as it should.
- **Retail:** the user confirmed a seated ped stands up before it leaves the seat. `motiongraph_sit.xml` [data, b84]
  leaves SittingCyc to Sit2Stand on "HasIntent StandUp" or "not HasIntent Sit".
- **Cause (ours):** the Sit motion holds the sit cycle until the packet's `StandUp` stage (`plugin_motion.rs`). The
  seat-drop path (`peds.rs`, "(dropped)") released the waypoint but never let the body leave the hold.
- **Change:** on that drop, a ped still in the sit cycle marks its plugin motion run orphaned, the same path as a
  packet that ends mid-clip, so the hold releases and Sit2Stand plays (`PED_PLUGIN_MOTION ... seat dropped while
  seated, standing up`).
- **Verification:** committed and pushed with #52 (the full skate-game suite passes on every later push); not play-tested.

### Follow-up: the plugin graph ends its behaviours (2026-10-19)

- **Retail [code]:** the Plugin behaviour's End `8269F310` calls the ped's plugin vfuncs +0x1C `82E0CFA0` and +0x20
  `82E3AEA8`. `82E0CFA0` loads the plugin's graph controller (`plugin+0x30`, object at `+0x1C`) and calls its vtable
  slot +0x10. The controller vtable `0x82321EA8` reads `82C12F48 82C131B0 82C13260 82C133D0 82C13498 82C12E70`
  (Ghidra dump), so +8 is Update `82C13260` (what the Plugin Update `8269F248` calls) and +0x10 is EndAllBehaviours
  `82C13498`. Every active plugin-graph behaviour runs its End: OwnPluginObject, IgnoreStandingCollisions,
  LockToCurrentPosition and the sit body's Create (its packet goes, so Sit2Stand plays).
- **Cause (ours):** leaving the Plugin state dropped `plugin_controller` without ending anything, so
  `ignore_standing_collisions`, `own_plugin_object` and `position_locked` stayed set and bumps were still refused after
  the stand-up.
- **Change (`peds.rs`):** before the plugin controller is dropped (Plugin state left) or replaced (another plugin
  graph), the old graph is looked up by its key and `end_all_behaviors` runs on it with the ped's brain
  (`PED_PLUGIN_END ped=#.. graph .. ended`). The orphaned-run path above stays as the fallback. The seat-drop log now
  names the condition: `(dropped: prop gone | streamed out | disabled | waypoint missing)`.
- **Tests:** `ending_the_plugin_graph_clears_the_seat_flags` (skate-core brain tests).
- **Open:** `82E3AEA8` (clears three ped slots tied to the plugin graph) is not ported, and what the slots mean is
  open. Whether a greet may preempt a seated ped in retail is not researched. Why the occupied bench dropped will show
  in the next session's log.

## The chase line on the HUD: "ZAP!", "Tazed!", "BAM!" (2026-10-19)

- **Ask (user, 2026-10-19):** port the taser's player-side feedback. Retail has no tased animation for the player
  (b248: the takedown wipeout); its one extra is a HUD line.
- **Retail [code, Ghidra]:** the hit calls the chase manager's slot +64 `826B4FC0` with (1, 1) for a taze and (1, 0)
  for a takedown. It looks up the HUD component `"central"`, rolls `rand() % 101 * 0.01` and posts a language id
  through `82663F38`: taze `ID_CHASE_TAZER_1` "ZAP!" below 0.5, else `ID_CHASE_TAZER_2` "Tazed!"; takedown
  `ID_CHASE_CAUGHT_1..4` ("BAM!", "Clobbered!", "KAPOW!!", "&@%#!!!") by quarters (thresholds 0.25 / 0.5 / 0.75 read
  from the image); first argument 0 is `ID_CHASE_OVER` "Freedom!". `82663F38` stores the formatted text, a style
  (`+0x34`, 0) and an audio hook (`+0x280`, 0) and sends the screen event 4.
- **Retail [data]:** the component is the `hud2/hudcentral` movie (we did not convert it before). Its `SetState(4)`
  calls `ShowMessage(mcMessageText, HUDComponents.Central_GetMessageData(), "center", ..., Central_GetAudioHook())`;
  `ShowMessage` writes `data[0]` into the label, colours it off-white for style 0, red for 1 (else the celebratory
  banner), plays `FE_SOUND_CENTRAL_MESSAGE_DEFAULT` for the hook `kAUDIOHOOK_DEFAULT` (= 0, defined in the movie)
  and the clip's intro. The getters' bodies are not decoded; their values are what `82663F38` stores [inferred].
  `Audio.PlayGenCentralMsge`, which the clip calls, is not in the TU3 image: a no-op in retail too.
- **Change:**
  - Setup: `tools/prepare_hud.py` converts `hudcentral` next to the trick display (`runtime/hudcentral.json`, now with
    the movie's imports / exports); `install_prepared_hud.py` installs it when present. A missing file is an
    `ASSET_CHECK hud` finding ("central message HUD missing") that offers Refresh Game Files.
  - Engine: `hud_central.rs` runs the movie on the APT VM (bindings above, FE sounds as `FE_SOUND_<NAME>`, the
    screen manager's two colours); `scoring_hud.rs` loads it with the HUD, resets it on a map change, advances it and
    draws it above the trick display. `CentralMessage` posts a line (`HUD_CENTRAL message ...` in the log).
  - `apt_movie.rs`: an import no library resolved is not placed (retail's null slot), instead of failing the load
    (the movie's team logo loader imports `Image_Renderer_Component`).
  - Living world: `post_chase_messages` turns a taze or a successful takedown of the local player into the retail id
    (`skate_core::...::chase::chase_message`, `PED_CHASE_MESSAGE` in the log).
  - Mods: `sdk.ui.central_message(text)` (command `ui_central_message`): a language id or plain text.
- **Tests:** `chase_messages_follow_retails_bands` (skate-core), `a_taze_line_shows_its_text_and_plays_the_default_sound`
  (data-gated: load, show, label text, the default sound, 90 frames), tool tests for the HUD install.
- **Open:** who sends the chase-over line (first argument 0); the line's on-screen look is not play-tested.
- **Fix 2026-10-19 (laptop session 10:16, user: "clobbered text appeared after the ped got mad and attacked me, but
  it never went away and was left on screen"):**
  - Retail [code]: the post `82663F38` resets the shown-message counter (`+0xBC` = 0) and sets the active flag
    (`+0xC0` = 1); the central HUD update `82664550` adds one per update and at 180 (`0xB4`), or when `82663E40`
    says the HUD cannot show, clears both and sends the screen event 5. The movie's `SetState(5)` calls its hide
    function on `mcMessageText` and `mCelebratory`, which plays "outro" unless `mIsSetToStayUp` [data]. Ours never
    sent 5, so the clip stayed on its idle loop.
  - Change: `hud_central::Runtime::tick(dt)` (called every HUD advance) sends `SetState(5)` after
    `DISPLAY_SECONDS` = 180 / 30 = 6 s (time-based: the console's 30 updates a second, same at any frame rate);
    `HUD_CENTRAL hidden` is logged. A mod sets its own time: `sdk.ui.central_message(text, seconds)` (0 to 60 s).
    The label gets the localised text, as retail stores it (`8290C8A8`); the renderer already localised ids
    (`apt_movie.rs` `text_changed`), so this did not change the look.
  - "Clobbered" was the line itself: `ID_CHASE_CAUGHT_2` is retail's "Clobbered!" (the takedown lines are BAM!,
    Clobbered!, KAPOW!!, &@%#!!!), so the text was right; only the hide was missing.
  - Test: `a_taze_line_shows_its_text_and_plays_the_default_sound` (data-gated) now checks the localised label, the
    hide at 6 s and the outro frames.

## The alert over a ped that warns or chases you (2026-10-19)

- **Ask (user, 2026-10-19):** "the ped alerts that appear over a peds head when they are mad (yellow) and when they
  are chasing/tazing (orangeish red). the notification is kind of 'sim style'".
- **Retail (research b191, 2026-10-17; re-checked in Ghidra 2026-10-19):** a screen-space HUD sprite, retail's
  "ChaseArrow" manager. The graph op `SendChaseStateMessage` (`826A77A8`) posts {ped, state} for a player target:
  warn (from `pedestrian_dowarning`), chase (intercept / secondary chaser), tired, giveup, end. The handler
  (`825E35F0`) keeps up to 10 entries (new ped added, known ped's state replaced and timer restarted, end removes).
  The update `825E2738` (decompiled again this pass) keeps an entry while `ms + dt` stays under 6000 ms for warn and
  3000 ms for giveup (chase and tired stay), drops a ped that is gone, projects the anchor with the view-projection
  and scales by camera distance (1.0 under 1 m, 0.4 from 50 m, linear between). The render draws a 64 px sprite
  per state when the point is in the centred 0.85 box, else an edge arrow on a ring of `0.85 * width / 2 + 20` px.
- **Retail [data]:** `arrow_warn` (yellow-orange), `arrow_chase` (red), `arrow_tired` (teal), `arrow_giveup` (grey)
  in `fedynamic.big`, 128 x 128 down arrows (colours checked from the decoded textures this pass).
- **Ours:**
  - `skate_core::living_world::peds::chase_alert`: the list, lifetimes, scale and on-screen / edge placement, all in
    `AlertSettings` with the retail values as defaults.
  - Setup: `tools/prepare_chase_arrows.py` exports the four textures (`private/hud/chase-arrows`, in the HUD stage;
    missing = `ASSET_CHECK hud` "ped chase alert textures missing", which offers Refresh Game Files).
  - Engine: `living_world/ped_alerts.rs` feeds the list from the existing `PedEvent::Chase` `state_*` events for
    the local player (`PED_ALERT ped=#.. warn` in the log) and draws one UI image per alert: bottom centre on the
    anchor over the ped, or rotated at the screen edge toward it. Hidden with the HUD (front-end screens, the PC
    overlay).
  - Mods: `sdk.ui.chase_alerts(false)` hides them (released when the mod stops); `ped_chase` events as before.
- **Not retail yet (ours):** the anchor. Retail uses the translation of the matrix the entity's vfn 0x28 returns;
  that getter is not decoded (the ped vtables' slot 0x28 are not it: the world table holds another class). The user
  saw the alert over the head, so ours anchors 2.1 m above the ped's root (`PedAlerts.anchor_height`).
- **Tests:** `states_replace_restart_and_end_like_retail`, `warn_lasts_six_seconds_giveup_three_and_ten_at_most`,
  `scale_and_placement_follow_retail` (skate-core); tool tests for the HUD stage; a muted run loads the textures.
- **Open:** the anchor getter; the gate global in `826A77A8` (vfn 0x9C, ours: not in zombie mode); not play-tested.

## Bumped peds warn before they chase; an attacker does not fall from its own takedown (2026-10-19)

Problem (laptop session 2026-10-19 11:05, notes game-20261019-110510.notes.log): "had a hard time just getting the
yellow warning to show. Pissed it off to the point it went straight to red and attacked me." and "the ped shouldn't
fall so easily from its own attack against the player; it can happen but it seems to happen every time I test it."

Root cause / evidence:
- Warn (research b316, b318; main checked the data and our code): the mood rows decide by the collision count on the
  skater's record, `skatercollisionwarn` (count 2, priority 2) before `skatercollisionchase` (count 3, priority 1);
  the producer skips a busy ped (`82E3BF70`), and a ped in a plugin is busy (`*(ped+96)` = the participant's plugin
  instance, b318). Retail's Plugin state goes to Colliding on a bump (`pedestrian_plugin.xml`, unless
  IsPluginCollisionOverride) and its End (`8269F310`) releases the plugin (`82E0C9E8`: leaves the conversation,
  aborting an unfinished one, and clears the instance); Colliding goes on to Wander (`pedestrian_colliding.xml`).
  Ours only cleared `in_plugin` at Plugin End, so the ped walked back into its conversation, stayed busy through
  every bump and its first producer run saw count 4 to 6: straight to chase (#4: six bumps, then chase at 17:06:41).
- Takedown (research b317; main checked the graph data): AttemptTakeDownTargetable and WaitForTakedownDecision are
  not interruptable and have no Colliding transition (`pedestrian_attempttakedowntargetable.xml`); the contact
  callback `82E38FB8` only stores the reaction and sets the begin-colliding byte, the touch decides the takedown
  (`826A4BE8`) and success clears the byte (`826A4E50`). Ours played the reaction at the contact: #4 entered
  WaitForTakedownDecision at 17:06:44.206, its own lunge read as a Knockdown at 44.276, the takedown succeeded at
  44.423.

Change:
- `brain.rs` Plugin End queues the host's ExitPlugin release (conversation leave / abort, prop and waypoint release).
- `peds.rs` drops a contact from the takedown target while the takedown is active (no reaction, no hit event; the
  takedown's touch test still sees it); `brain.rs` TakeDownTargetableSuccess clears `begin_colliding`.
- Ported 2026-10-19 (section "A failed takedown keeps its stored reaction, and may trip"): after a FAILED takedown the stored reaction plays once the graph reaches Colliding.

Verification: skate-core `leaving_the_plugin_state_releases_the_plugin`, `a_takedown_attempt_succeeds_on_contact_with_the_target`
(now also: success clears begin_colliding); skate-core 1132 and skate-game 817 pass. Needs a play test: bump a
conversing ped twice (yellow warn on the second), a third time (chase), and let one take you down (it stays up).

Open: the branch that sets brain+3277 bit 0x04 in Colliding Begin (`8269DA00`, `ori 4`); the failed-takedown delayed
reaction; recomp traces proposed in b317 / b318 (not run).

## A failed takedown keeps its stored reaction, and may trip (2026-10-19)
- **Problem:** ask 78 follow-up: during a takedown attempt ours dropped every contact (the target's touch decides the
  takedown, correct; anyone else's contact was played at once or lost), and a failed takedown never tripped.
- **Retail** ([code] Ghidra + recomp, `.local/research/b326-failed-takedown-reaction.md`; design
  `.local/research/b329-deferred-reaction-port-plan.md`): AttemptTakeDownTargetable Begin `826A49D8` clears the
  collision component's begin-colliding byte (+8), so a reaction stored before the attempt is dropped, and copies the
  chosen entry's byte +38 bit 0 ("may trip"). Its Update `826A4BE8`: a contact by anyone but the target blocks the
  decision (the graph's 3.0 s timer then fails it). TakeDownTargetableFailure Begin `826A5068`: trips (stores
  Knockdown / FromBack) when the target refused, or when the entry may trip and a 1..=100 roll is at most
  `tripChance` x 100 (default 0.35, `8269A588`); otherwise a reaction stored during the attempt stays stored. Only
  Success `826A4E50` drops it. The stored reaction plays when PedestrianColliding begins (`8269DA00`). Stock data:
  every takedown entry has byte +38 = 0, so a failed takedown on the player never trips in stock data.
- **Change:** a contact during the attempt (not by the target) is stored (`PedEvent::ContactStored`,
  `PED_SKATER_CONTACT_STORED`) into the brain's `pending_reaction` and arms Colliding; Attempt Begin clears it; a stored
  contact blocks the success decision [approximation: retail looks at the last toucher]; Failure Begin trips or keeps
  it; Colliding Begin moves it to `reaction_due`, `think_peds` hands it to the body and `advance_peds` plays it in the
  same fixed tick (`PedEvent::DeferredHit`, `PED_SKATER_CONTACT_DEFERRED`; mods get `ped_hit` with `deferred: true`;
  not a `Hit`, so it does not arm Colliding again). Data: `TakedownEntry::trips` (byte +38 bit 0 of the table word),
  `tripChance` from the graph (`TakeDownTargetableFailure`), `BrainSettings::trip_kind` / `trip_direction` (retail
  Knockdown / FromBack). The roll is the brain's deterministic per-ped RNG, only drawn when the entry may trip.
- **Files:** `crates/skate-core/src/living_world/peds/{brain.rs,skater_contact.rs,takedown.rs}`,
  `crates/skate-game/src/living_world/{peds.rs,ped_mood.rs}`, `crates/skate-game/src/modding/living_world_events.rs`.
- **Tests:** `a_failed_takedown_keeps_its_stored_reaction_and_may_trip` (default 0.35, Attempt Begin drops an earlier
  one, the decision is blocked, failure keeps it with no roll, Colliding hands it over, a trip replaces it).
- **No target refuses a takedown (2026-10-19, b340):** the target interface's slot 0x10, which the failure path asks
  first, is `8281DD70` (`li r3,1; blr`: accept, read in the recomp code [code]) in every vtable that carries the
  interface (seven, from a memory search for its slot 0x28 `8258E6C8`; the four constructors that register it all
  start from base vtable `8230065C`). So retail's refusal branch never runs, and ours (`refused = false`,
  `peds/brain.rs` TakeDownTargetableFailure) has the same outcome. Which classes own the five non-ped vtables is not
  identified; it does not change the result.
- **Mod keys (2026-10-19):** `ped_brain.trip_kind` ("Knockdown", retail, or "Standing") and
  `ped_brain.trip_direction` ("FromBack", retail, "FromFront", "FromLeft", "FromRight") set the reaction a failed
  takedown's trip stores (`BrainSettings::trip_kind` / `trip_direction`); unknown names fail the patch's validation.
  Tests: `trip_reaction_names_round_trip_for_mod_data` (skate-core) and the skate-mods patch validation.
- **Open:**
  retail's last-toucher rule; brain +0xCCD bit 0x80 (set during the failure) has no reader we know.
## Props stop against peds: body A, the movement proxy (2026-10-19)

**Problem.** Props passed through peds: a bench or bin shoved into a ped slid on as if nobody stood there. Our ped
body (body B, three capsules in group 11) is only in the skater's solve; the prop step never saw a ped (research
b342, `ped_body.rs`, `prop_dynamics.rs`).

**Retail [code, b342; trace, main's own spawn-only recomp run].** A ped has two bodies in the world's ped pool:
body A (group 9, one upright capsule, radius 0.3125 m, half length 0.5375 m, 10 kg, posed from the root,
`sub_82E38400` / `sub_82E35160`) and body B (group 11, posed from the skeleton). Every pair job filters body pairs
through one collision-type bit table (`40122C30`, width 21), which a new recomp hook (`hooks_pairtable.cpp`,
category `pairtable`, skate3recomp research-hooks) dumped in a spawn-only run in DownTown: the decoded table is
symmetric, and body A collides with the static map (0), cars (8) and DMOs (12..14) but never with the skater (4..7)
or another ped (9..11); body B collides with the skater and with other peds' body B, never with the map, cars or
DMOs. Contacts on body A of kind 1 (the ped's own plugin object) and 2 (a car) move the ped root; any other contact
leaves the root and body A is re-posed from it next frame. Matrix: `.local/research/b342-pairtable-matrix.txt`.

**Change.** `ped_body.rs`: `PedProxy` (retail defaults, `PedBodySettings::proxy`, on), `body_a_id` (bit 47, so it
never equals a body B part id and still names the ped), `body_a_volume` (upright capsule standing on the root,
moving with the ped). `push_ped_proxies` writes every near ped's body A to `GamePhysics::ped_prop_volumes` (its own
list, so the NPC proxy system's clear of `actor_prop_volumes` cannot drop it); `step_props` passes them to the prop
step after the skaters, and `PropDynamics::set_actor_push_mass` gives them their 10 kg in the pusher rule
(`push_from_skater`), so a prop moving into a ped is slowed by a 10 kg contact every tick and stops against it. The
skater solve keeps only body B, which matches the table (the skater never meets group 9). Mods:
`living_world.ped_body.proxy`, `proxy_radius`, `proxy_half_length`, `proxy_mass`.

**Not retail yet / open.** Retail solves body A and the prop in one shared row solve; ours uses the pusher rule the
skater volumes use (same as skater x DMO). The capsule's height offset (`sub_82E35160`, tuning +0x2C) is not decoded
(ours stands it on the root). Cars still push the root through `vehicle_contacts` (retail's kind 2 does it through
body A, same outcome expected); kind 1 (the root following a ped's own plugin object) is not ported. Ped x ped
contact stays the walker's separation (retail's 11 x 11 body B pair does not write the root either).

**Tests.** `body_a_is_an_upright_root_capsule_in_group_9_with_its_own_id`, `a_sliding_bin_stops_against_a_peds_body_a`
(a 4 m/s bin slides past the spot without the ped and stops short of it with one), skate-mods patch validation for the
proxy keys. Not play-tested yet.

## A seated or prop-using ped can be knocked over by the skater (2026-10-19)
Research b371 (`.local/research/b371-ped-reaction-retrigger.md`) [code]: our contact gate already let a ped through
while its plugin or taunt clip played (retail skips a skater contact only while the ped is in a collision reaction),
but the hit then called the animation player's `react`, which refuses while any reaction runs, and plugin and taunt
clips run as reactions. So a ped on a bench, at an ATM or in a taunt was never knocked over or stumbled.
Change: in that case the hit uses `interrupt` (the path the Avoid graph's SitJump already takes) and ends the plugin
or taunt clip's state. Not play-tested. Also from b371: stumble re-trigger needs no change (ours 0.80 s minimum, median
1.33 s in 47 laptop gaps; retail samples 0.93, 1.30, 2.10 s); retail's anim-set timer (`ground_seconds`, 1.5 s, 0.5 s
for security01) drives only the knock-down's ground state machine (re-armed by a contact at expiry, ground
re-reactions, blocked after 5 expiries), which ours does not have yet (follow-up, ped physics todo).

## A knocked-down ped gets up at retail's time (2026-10-19)
Research b372 (`.local/research/b372-ped-knockdown-state-machine.md`) [code + data]: retail's knock-down Begin
(`sub_8269DA00`) starts the anim set's ground timer (`ground_seconds`, 1.5 s; 0.5 s for security01) with the fall,
and `sub_8269E058` posts the `Recover` intent when it runs out; the motion graph holds the ground cycle until then. So
the get-up comes at the later of the fall clip's end (0.967 s) and 1.5 s after the hit: about 0.53 s on the ground.
Ours counted the 1.5 s from the start of the ground cycle, keeping a ped down about 0.97 s too long.
Change: `PedAnimPlayer` counts the ground time over the whole reaction (`anim.rs`), test
`a_knock_down_falls_lies_for_the_ground_time_gets_up_and_walks_on` updated (get-up 1.5 s after the hit). Plugin
holds (infinite time, released by `release_hold`) are unchanged. Not play-tested.
Still to port (b372 port plan): the rest of the Colliding state machine (a contact at expiry re-arms the timer,
re-hits while down or getting up pick LyingDown / Crouched / Standing by `recovery_intervals*`, contacts blocked
after more than 5 expiries); the reader of `[ped+0x1714]+0xD4` is not found. b372 also corrects the b156 addendum:
LyingDown keeps the stored Begin direction, Crouched and Standing take the new hit's direction.
Recomp measurement (2026-10-19, `.local/research/b359-ground-probe-hook-plan.md` section 9) [trace]: retail's ped probe
length L is 1.6 m (probe settings +16, every call seen; the retry scale 4.0). A hit and the mover update's calls were
not captured (the hook crashed the recomp during the teleport's level load); not needed further, since the user confirmed retail peds always stand on the floor.

## Peds get hit again while down, and stand on the floor everywhere (2026-10-19)
- **Knock-down state machine (b372, b373)** [code + data]: a knocked-down ped now follows retail's PedestrianColliding
  states. A contact while down waits for the ground timer's expiry and re-arms it (stay down another
  `ground_seconds`); a contact while getting up re-hits from the posture the get-up clip has reached
  (`sub_8269E460`: before `lying_down_until` lying down, the fall restarted at the set's ground restart time and the
  stored direction; before `crouched_until` the crouch clip; else the whole fall; the last two take the new hit's
  direction and reset the expiry count); contacts are dropped once the timer has run out more than `block_after`
  (5) times. All values per anim set from the export (`recovery_intervals*`, `Wipeout*GroundRestartTime`, crouch
  clips), retail as defaults; `block_after` is ours as data. Log `PED_COLLIDING`. Test
  `a_knocked_down_ped_stays_down_when_hit_again_and_is_rehit_while_getting_up`; data test checks the per-set values.
  Not play-tested. Open: the reader of `[ped+0x1714]+0xD4`.
- **Ground probe (b359, b358)** [trace]: the drawn ped's ground cast now uses retail's measured probe length 1.6 m
  both ways, then the 4x retry with retail's shape (a cast up to find a ceiling, then down from below it). Before,
  the upward reach was 0.2 m, so peds on navmesh up to 4.3 m under a road were drawn sunk; user 2026-10-19: "Peds
  dont fucking sink into the ground.. ever.." (retail outcome). Not play-tested; `PED_FLOATING` logs any gap.

### Warn skipped after bump 2 (laptop session 2026-10-20, research b393; main checked the log and code)
- Log: #281 was bumped three times (16:37:59, 16:38:00, 16:38:03) and chased on the third, count >= 3 as retail;
  #126 the same. After bump 2 the producer picked `converse_everyone` (priority 2, presence records ahead in slot
  order) over `skatercollisionwarn` (priority 2): the tie keeps the first hit, as retail's `82E41EB0` (`priority <
  best`). The line was held back by our log repeat limiter, so the log looked like a first-bump chase.
- Difference from retail: `PedBrain::busy` lacked the colliding term (`brain+3277` 0x04, `82E3BF60`), so the producer
  ran one tick into Colliding. Change: `busy()` ORs `colliding`; PED_MOOD always logs collision results and any change
  of result per ped. Open: retail's record slot order (`82E41060` insert, expiry erase), whether retail posts presence
  records about other peds like ours.
- Water bail (b392): retail has no water or respawn chase end; ours matches by code (5 m unreachable bit, 65 m escape,
  capture within 2 m while wiped out). Open: a virtual call from the reset path; the chases-off window bit
  (`826B4C70`, G+6332 0x04).

### A slowly moved bench re-sat its ped a dozen times (laptop session 2026-10-20 11:21, b396, b219)
- User: "after bumping the female ped near the on that attacked me and failed to reach me, she tried to resit about a
  dozen times before giving up and walking off." Log #20: SitForever -> TargetWaypointPrecise -> EnterInteraction ->
  SitForever ~12 times in 3 s (seat distance 0.046 to 0.052 around OnWaypoint's 0.05 m) until the bench was disabled.
- Cause: b219's small-step carry was never ported. Retail sums a seat's steps at or under the 2.5e-5 m^2 gate into the
  ped's root (brain `+3168`, `82E36D70`), so a slowly pushed bench carries its seated ped; ours held the body on its
  lock while the seat crept away. (The want itself is retail: Plugin has no nearbycollisionreaction exit, b396.)
- Change: `peds.rs` `seat_carry`: sub-threshold seat steps move the seated owner's lock and body with the seat.
  Open: b219 plan step 3 (latched avoid ahead of the stand-up) and the `seated` test needing the SitIdleCyc clip.

## A bail from a ped hit is logged, and the mid-trick factor is tuning (2026-10-21, research b448)

- Retail (b185, re-checked by b448): no ped-specific bail; the ped body parts' contact force (group 11) goes through the
  ordinary wipeout checks (ground 8.5 body / 30 arms / 0.4 displacement / 0.6 squash, air 3.5), whose limits shrink by
  0.3 while the board is side-on mid-trick (flags_2472 bit 15, set by 82BD8CB0 at |board X.y| > 0.71). The ped's
  reaction is a separate read of the same contact. Ours already did this (doc sections above).
- New: `WIPEOUT_REASONS` logs `ped_force` (the strongest group-11 force), `side_on_trick` and `trick_scalar`, so a
  session log shows whether a bail came from a ped; the 0.3 factor is `GroundSettings::trick_scalar` (retail default,
  a code constant in 82D8F9E0, now mod-tunable). Test `a_ped_hit_bails_side_on_mid_trick_but_not_upright`.
- Open: retail's bail rate per ped hit (a recomp ped-hit session with the b274 WIPEREQ hooks); a board play test.

## Code reference: living_world/peds/brain.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/brain.rs`; each comment there links its section here.

### Ped brain

A pedestrian's behaviour brain: the host the stock ped AI graph (`Pedestrian.stategraph`) runs on (doc 26 "Ped behaviour runtime"). Retail runs that graph on the same dynamic controller as the skater's graphs (`82C12E88` via `82C12DD0`; `.local/research/peds/b2-ped-aigraph-interpreter.md`), so the shared `graph::controller` drives it here too; this module owns what the graph's operations read and write: wants, timers and flags (the retail brain, `brain+412` wants, `82E40940` / `82E40A80` timers, `+3232` honker, `+3281` bit 0x80 scatter) and the outputs the body follows (motion intent, speed suggestion, flee target).

Operations are parsed once from the graph's attributes ([`PedOp::parse`]); their values stay data. Operations not ported yet are [`PedOp::Pending`]: their conditions answer false and their behaviours do nothing, and the game reports them once (no silent default). Retail bodies (`.local/research/peds/b3-first-slice-operations.md`, main checked Wander and the warn timer): see each variant.

Multiplayer: the brain is plain data owned by the host; wants carry stable target ids.

### Send chase state

`826A77A8` Begin (factory `826CB3B8`, `stateName`: warn 0, chase 1, tired 2, giveup 3, else 4): not in zombie mode, tell the chased player (the chasee, or for `warn` without one the flagged warn want's target) the ped's chase state (message `0xEFFA9715`: the marker arrows). No speech.

### Takedown taunt victim

The mood gate's flags (`brain+3278`): WaitingToReact 0x20 (set by the producer on a pass and by these ops, cleared when a want group begins) and IsReactingToMoodEvent 0x10. `826A70E0` Begin: the taunt want's target (want 3); speech 65 when the victim is the player, else 19 (host); face the target; post the "Taunt" motion intent (motiongraph_taunt: the remapped "Taunt" clip once). The graph holds the state while the intent lives (`HasMonitoredIntent SGIntent`). `826A72F0` End: face released, intent removed, the taunt want unset.

### Starting hand prop

The ped's starting prop (ped constructor `82E33198` [code]): `chance_roll` and `pick_roll` are retail's two `rand() % 100 + 1` rolls (1..=100). The ped carries something when `chance * 100 >= chance_roll` (`8269A588`, the entity's `Hash_3DB019A08284F45C`); then the first entry of its `handprop_odds` list whose running total x 100 reaches `pick_roll` (weights are not normalised: a list summing below 1 can give nothing).

### Tazer clip

The ped update's tazer step (`82E3ED50`) [code, b153 / b248]: `played` is the tazer clip the channel last took (`TazerClip`) and whether the ped's anim set had it, `left` the seconds left on the channel. Drawing: with `tazer_clip_end` left on the draw clip the cycle clip starts and the tazer is drawn (state 2). Putting away: with `tazer_clip_end` left on the holster clip the draw request clears, state 0, and the "tazr" prop is deleted (not thrown). A clip the set lacks counts as already at its end.

### Unset want

`sub_82E42A58(brain, want, 0)`: a set want (0x80) without flag 0x40 counts one finished reaction on its target's perception entry (`+68`, when the entry exists); a flag 0x40 want instead forgets its target (`sub_82E40FF0`: the mood records about it, game side through [`ChaseRequest::MoodReset`], and its perception entry).

## Code reference: living_world/peds/mood.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/mood.rs`; each comment there links its section here.

### Ped mood

A pedestrian's mood: what happened around it (the skater nearby, a collision, a trick) and the want producer that turns it into wants for the ped AI graph (doc 26 "Ped behaviour runtime"). All rules come from the stock mood tables (`livingworld_moodeventcategories`, `livingworld_entities_moodreactions`, `livingworld_entities_moodresults`, exported by setup); this module holds no values of its own.

Retail (TU3, evidence only; re-implemented; `.local/research/peds/b4-ped-want-producer.md`, `b5-ped-mood-events.md`, `b6-ped-mood-fields.md`; main checked the key addresses):
- posting `sub_82E41060`: an existing record with the same (instigator, second entity, category) is bumped (`sub_82E41750`: magnitude += the category's per-event magnitude, count += 1, age = 0, position = the event's), else a new one is pushed (`sub_82E414D8`: at most 10; when full only an event without an instigator or second entity may evict one: `sub_82E441C8` takes the record whose category Priority is numerically highest, the oldest on a tie, the first in slot order on an exact tie; b189, main re-read the compares). The per-event magnitude is the category field `B1DD` (the presence period, 0.5 s stock; discrete categories have none: the attribute default `0x830D0850`, 0.0);
- per tick (`sub_82E41600`): cooldowns run down and go at 0, ages run up, suppress timers run down to 0; magnitude and count never decay; a record older than its category's lifetime (`A379`, 30 s stock) goes (`sub_82E440E0`);
- presence (`sub_82E3CA20`): every presence period each player within 35 m (`0x8206D148`) posts `presence` (instigator = player, second = the ped);
- the producer `sub_82E41B98` / evaluator `sub_82E41EB0`: for each record not suppressed, the ped's reaction results by priority (lower wins), each checked in retail's order (see [`MoodTables::evaluate`]); the winner is rolled (`sub_8269A588`: `rand() % 100 + 1`, pass when p x 100 >= n; p 0 always passes), every roll starts the result's cooldown; on a pass the result's wants are raised on its target (instigator, second entity or the ped itself) and the record is suppressed for the result's suppress time. Not decoded (answer false, documented): the target checks `vfunc +28` / `+32` (`D9F3` / `D44E`), the hand-prop bit (`brain+3278` 0x02, `072D`), the ped-state prerequisites (kinds 1, 2, 4, 7, 8), the sight test (`sub_82E27240`; every post is seen). The count range (`7FF7`) reads the instigator's perception entry `+68`, raised wants about it (inferred).

### Mood prerequisites

Prerequisites of a record's category (`sub_82BFED98`, AND of the items; each item `sub_82BFEA60` about the ped itself) [code]: kind 2 OwnPluginObject, 3 the squared distance from the ped to the record's position (`+48`), 4 the ped is chasing, 6 the second entity is the ped itself (`sub_82E41060`'s input), 8 the `flee` want (slot 10) is pending, 9 zombie mode. Comparators: 0 the value, 1 its negation, 2 farther than the item value, 3 nearer; any other is false. Only kind 3 has a distance (0.0 otherwise) and only kind 3 squares the item value. NOT RETAIL YET: kinds 1 (`0x8269A888` vfunc +16), 5 (`0x8269A950` vfunc +8 with the position) and 7 (the chasee's `0x8258E778` vfunc +12) are false (research b152).

## Code reference: living_world/peds/plugins.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/plugins.rs`; each comment there links its section here.

### Ped plugins

Ped plugins on world props: benches, trash bins, newspaper boxes, vending machines, ATMs, water fountains, look-at and spectate spots, gathering places (retail TU3, evidence only; re-implemented; `.local/research/peds/b81-ped-plugin-acquisition.md`, `b82-ped-plugin-search.md`; main checked the ped roll `82E3B060` and the d100 roll `8269A588`):
- a plugin prop owns waypoint groups, one waypoint each for DMO hotpoints (`82C4E128`: hotpoint type 1 = trash bin, 2 = newspaper box, 0 = the root (no waypoint), 8 skipped, any other type = a seat) or the placed groups of the world (`waypoints.json`: vending machines, trash bins);
- a ped type's `plugin_odds` (attribute `F89323E420A6AAA3`, a list of {prop class, probability}) drives two things: the ped's own yes / no when a prop is offered (`82E3B060`: odds x 100 >= `rand() % 100 + 1`; a refusal remembers the prop, `82E40BD0`, at most 16 entries with the value 30.0) and the spawner's weighted pick of a ped type to spawn at a free prop (`826B90F8`: weight = group weight x odds, draw = rand x 2^-32 x total; on success the prop's cooldown `+232` restarts from `+228`);
- a descriptor takes one participant unless it says otherwise (`82C17AF8`: `maxNumberOfParticipants`, default 1).

Multiplayer: [`PluginProp`] and [`RefusalMemory`] are plain host-owned data; every roll takes the host's seeded RNG.

### Plugin transfer conditions

One transfer condition for `ped` at `prop`: `HasHandProp` (`826AD1A8`), `HasDisposableHandProp` (`8269D170`), `CanSitWithHandProp` (`8269D268`), `IsChasing`; `IsOnRoad` is false (its condition slot is the `li r3,0` stub `8274CA90`). `HasFirstWaypointAvailable` (the prop's waypoint 0 is free) and `InFrontOfFirstWaypoint` (`8269CF58` [code]: the ped within `front_half_angle` of waypoint 0's flat facing; a near-zero vector counts as angle 0) are ported. Other names (`IsNearCrossWalk`, not ported) answer false like the brain's pending conditions.

### Plugin prop check

`82C56DB0` on one prop: `authored` / `now` = (position, basis columns). Moved farther than `max_distance` from its authored spot (|d|^2 - max^2 > 0) -> off. Every rotation element within 2^-16 of the authored -> probe the seats. Otherwise the relative rotation as a rotation vector (`8296F1A0`: axis x angle, radians) is compared per axis with `tolerance_deg` x 0.0174533 (`0x8206D110`); any axis over -> off. NOT RETAIL YET: the volume-box test (`*(0x83085494)` vf+144) is not ported, and the frame of the relative rotation (`8286CAF0`'s operand order) is taken as world (now x authored^T).

### Plugin prop offer

One prop's offer in the scan (`826C0058` -> `826BFE18` for every ped, registry order, no early exit): a ped without a plugin, not refusing the prop, within `height_tolerance` vertically and `radius` horizontally, accepted by `qualifies` (the offer's own test, vfunc 10) and passing its `plugin_odds` roll takes the prop's nearest free waypoint (vfunc 14); taking it also remembers the prop for 30 s (`8269F248`). NOT RETAIL YET: the offer's vfuncs 18 / 9 / 10 are not decoded (ours: a free waypoint and below the descriptor's participant cap, then `qualifies`); the radius source is open (ours: the class field `E2101F2B17A0E6B5`, [inference]).

### Conversation offer

A conversation area offered to one ped (`826BFE18` with the conversation offer `0x8232B798`, b212): a ped without a plugin, not refusing the area, within `height_tolerance` vertically and `radius` horizontally of its centre, passing the descriptor's transfer expression (`qualifies`) and its `plugin_odds` roll for [`CONVERSATION_CLASS`] attaches (`82E1BF00`); the add (`82E1D380`) can still turn it away. Used by the scan for passers-by and by the spawner for the founders (`826A6CD8`). Joining remembers the area like a prop.

## Code reference: living_world/ped_body.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/ped_body.rs`; each comment there links its section here.

### Ped solid bodies

Peds as solid bodies in the skater's contact solve (doc 26g, "Peds are solid"; research b138, b144 to b147).

Retail: a ped is a body in collision group 11 that the skater's skeleton contact traversal (`sub_82BD4A30`, group switch at `0x82BD5388`) solves against like a car (8) or another skater (5): the solver pushes the two apart, the group-11 force goes to `+4060`, and every contact also feeds the per-region strongest contact the wipeout checks read, so a hard or mid-trick hit into a ped bails the skater through the ordinary thresholds [code, b138 finding 1]. The ped's own reaction (`sub_82E38FB8` kind 5: knock-down / stumble) stays in `peds::skater_hit`.

The body (group 11, `sub_82E268C8`) has three capsule parts, measured in the recomp (b146: radius, half length, mass) and posed every frame from the animated skeleton by `sub_82E35838` (b147, main checked the table builder `sub_82E33070`): part 0 at Hips, framed along Hips -> Head; parts 1 / 2 halfway between LeftForeArm / LeftHand and RightForeArm / RightHand along the forearm; every part moved ahead by the ped's velocity x 1/60 s. The parts are keyframed (rebuilt from the pose, no write-back found): a finite mass for the solve, no rotation from it. NOT RETAIL YET: part 0's axis is taken as Hips -> Head (retail builds a frame from Hips' third axis and that direction, then turns it 90 degrees; which way is open); body A (retail's group-9 movement proxy, b342) is not ported, body B stays in the solve through every reaction, as retail (b203); bodies only within [`PedBodySettings::range`] of an observer (ours: posing costs time). All mod-overridable (`living_world.ped_body`).

### Ped body A

Body A (retail `sub_82E38400`, group 9; b146 radius / half length / mass, b342): one upright capsule from the ped root. Retail pairs group 9 with the static map, cars (8) and DMOs (12..14), never with the skater or another ped (the collision-type table `40122C30`, read in the recomp, b342 section 7). Ours: props meet it through the pusher rule skater volumes use (`PropDynamics::push_from_skater`) with its mass; cars still push the root through `vehicle_contacts` (retail's kind 2 does it through body A).

Pushed props shove peds aside (2026-10-26, USER-ASKS 375 "When I hit a ped with the dumpster I came to a full stop, even though the ped was on the ground"; research b635, b636, b644). Retail [code]: body A is a finite-mass body in the solve, and the contact classifier `sub_82E38400` gives a prop touching body A kind 1 ("collision sliding", ped +5936 bit 0x40 from ped attribute `0xB168050B3E6FC967`: on for every generic ped type) when the contact is a side contact (`|up x n| > 0.45`, `0x8208E95C`) the ped faces into or moves into; the handler `sub_82E38FB8` then moves the ped's root to body A's solved X / Z (height kept, no stumble, no mood event). A prop never knocks a ped down (kind 0 on body B). Body A's pose (`sub_82E35160`): an upright capsule centred on the Spine1 bone's position (root rotation), so it drops with a knocked-down ped's spine; without a pose it sits at root + height / 2 + 0.2. Ours before: body A was rebuilt at the root every step with zero velocity, so it acted as a wall (the dumpster stopped dead). Ours now: the prop step reports each actor's solved body A displacement and velocity (`PropDynamics::take_actor_shoves`, the row reactions are displacements), `GamePhysics::ped_prop_shoves` carries them to `push_ped_proxies`, which moves a sliding ped's root by the shove when `slides()` holds and builds the next body A at the moved root with the solved velocity, centred on Spine1 when the rig has it (`PedProxy::centre_bone`, `unposed_height` 1.7 inferred, `unposed_lift` 0.2, `slide`, `slide_side`). Log line `PED_PROP_SHOVE ped=#<serial> by [dx, dz] at [...]`. Open: the contact normal's sign is inferred from logic (b636: it points into the ped); a ped hit from behind while walking away gets kind 0 and still stops a light prop, as retail's rule says; the per-type sliding attribute is not read (all ours are generic); the hold bits (0x40 / 0x80) and the +0.25 lift are not ported. Tests: `a_sliding_bin_shoves_a_ped_whose_body_a_follows`, `kind_one_needs_a_side_contact_the_ped_faces_or_walks_into`.

A carried prop meets peds too (2026-10-27, USER-ASKS 387: "it went through one of the peds, which shouldn't happen", then "trapped a ped inside the dumpster"). Cause [code + log]: `PropDynamics::step_with_actors` skipped every actor contact for the held prop (the carrier's pushes fight the drag), so peds and NPC skaters met nothing either; session game-20261027-073605 has no shove during any dumpster carry. Change: only the local skater (the carrier) is skipped for the held prop; peds and other actors get the contact pass and its shove. Test `a_carried_bin_shoves_a_ped_instead_of_passing_through`.

### Ped proxies in the solve

Add the near peds' bodies to the skater solve (after the other proxies; rebuilt every tick). The bodies stay through a stumble or knock-down, as retail's do: retail removes a ped's collision bodies only in the collision object's destructor (`sub_82E22D50`, ped despawn; research b203, laptop trace 175036), so a skater cannot roll through a ped that is already reacting. New hits during the reaction are still skipped (`peds.rs`, retail `sub_82E38FB8`).

## Code reference: living_world/peds/chase.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/chase.rs`; each comment there links its section here.

### Ped chase intercept

Ped chases (doc 26 "Ped chases"): the per-type chase record and where a chasing ped runs. Retail (TU3, evidence only; re-implemented; `.local/research/peds/b10-ped-chase-takedown.md`, `b11-ped-intercept-takedown.md`, main checked the constants and the angle source):
- each ped type points at a `livingworld_entities_chase` record (escape distance, exhaustion, run speed, max lead time, ...); the chase manager holds the `global` record (predict angle);
- InterceptChasee Update (`826A3BF0`) asks the solver (`sub_82E3D4D8`) for a goal: the flat intercept time `t` with `|Q + V t - P| = speed t` (`sub_82E15CD0`); no solution or `t` not below the record's max lead time -> no new goal this tick. When the angle between (chaser - chasee) and the chasee's velocity lies strictly between the predict angle and 180 deg minus it, the goal leads the chasee (`Q + V t`); else it is the chasee itself. The goal keeps the higher of the two heights, the nav runs at the run speed with a 0.5 m arrival radius (`0x8209975C`). Which root `sub_82E15CD0` takes when there are two is not fully decoded: we take the smallest positive one (inferred).

### Chase block prediction

`82D99540` UpdateBlockPrediction: does a secondary chaser at `chaser` block a chasee at `chasee` moving at `velocity`? Within the range (flat), either touching (`chasee_radius` + the block radius; the chasee radius `G+1648` has no known writer, 0.0 here) or the chasee runs at the chaser faster than the closing speed within the cone. The block point is then the formation point (the entry direction stays 0).

### Chase reach check

The mover's reachability check (`82E2CB08`, every 10 mover updates: `82C465A8` snaps the ped and its current destination onto the navmesh, `82C464F0` tests that both have a polygon and are connected; research b177 / b302). `ped` and `destination` are the located polygons (`None` = not on the mesh, e.g. a chasee in deep water). True when the check reports the destination unreachable (retail result codes 0, 2, 3, 4 post an event; 1 posts none). Ours: the reachable-but-high-area-class case (`82C47D60`) is not ported.

### Chase message

The line the HUD's central message shows when a chase reaches the player (`826B4FC0`, chase manager `0x830854B8` vtable slot +64): `caught` false = the chase ended (`ID_CHASE_OVER`); a taze picks by halves, a takedown the four "caught" lines by quarters. `roll` is retail's `rand() % 101 * 0.01`. Language ids: the text is the HUD movie's language table ("ZAP!", "Tazed!", "BAM!", ...).

## Code reference: living_world/peds/vehicle_contact.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/vehicle_contact.rs`; each comment there links its section here.

### Cars touching peds

A traffic car touching a ped (doc 26, "Cars hit peds"): detection and the ped's response.

What retail does [code, TU3; addresses are evidence only]:
- Every contact on a ped's collision body goes through the ped's contact callback `sub_82E38FB8` (ped vtable `0x8232BE80`). It first classifies the contact with vtable slot +164, `sub_82E38400`, which returns a contact kind 0..5 ([`RetailContactKind`]). The other body's owner (`[[contact+76]+32]`) is checked with the interface cast `sub_82965630`: an owner that is an `IVehicle` (type getter `0x82C34050` -> `0x823220B8`, string `IVehicle`) gives kind 2, with no speed or angle test.
- The callback's switch at `0x82E39230` sends kinds 1 and 2 to the same block (`0x82E3926C`): the ped's root is set to its collision body's position after the solver moved the body (body pose from `sub_82585CB0`, minus the body-to-root offset at `[ped+5756]+19872` when the ped's slot +180 says so), keeping the root's height (`vrlimi` keeps y), skipped when the result is not finite or out of range (`0x822F88D4`). Nothing else: no reaction kind or direction (`ped+2496` / `+2500`), no `Collision` intent, no speech, no brain flag (`+3196` bit 0x80 is set only for kinds 4 and 5). So a car never knocks a ped down, never makes it stumble, ragdoll or fade: the car's (infinite mass, kinematic) box shoves the ped's body out of the way and the ped walks on.
- Kind 5 (an `IActor` owner, `0x82586478` -> `0x823000F0`, e.g. the skater) is the knock-down / stumble path (3.0 / 6.0 thresholds, `peds-re.md` section 5); kind 4 is an actor contact on body part 1 or 2 (acted on only while `[ped+5756]+140` is 7); kind 3 sets `+3278` bit 0x80.
- The car side (`sub_82C3C150`, vehicle collision interface `+136`) records an `IActor` toucher in its "hit by" mask `+4248` and sets `+4401` bit 0x20 for a contact ahead of the car; it does not stop or honk there. Peds flee from cars only through the horn (`sub_82C40660` kind 2, the honked-at input, `RunFromHonker`; `traffic/horn.rs` sets the ped's honker, the RunFromHonker op is not ported yet).

Ours (stated, NOT RETAIL YET where marked): the car is our kinematic box (GLB bounds), the ped body is a vertical cylinder of the NavPower agent radius and height (retail's Havok ped shape, `sub_82E26430`, is not decoded); the solver's separation is the smallest push that takes the cylinder out of the box in the ground plane (Havok's own penetration recovery is not decoded). The game then keeps the pushed ped on its navmesh, standing in for the world collision the retail body has.

Deterministic: pure functions of the poses; the game resolves peds and cars in id order.

## Code reference: living_world/peds/conversation.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/conversation.rs`; each comment there links its section here.

### Ped conversation object

Ped conversations (doc 26 "Ped conversations"): the conversation object a ped spawns with SpawnConversationArea. Retail (TU3, evidence only; re-implemented; `.local/research/peds/b23-ped-plugins-conversations.md`, `b24-conversation-object.md`, main checked the vtable, the completion state and the spawn constants):
- a conversation is a waypoint plugin entity (ctor `82E1CEC0`, interface vtable `0x8232B868`) with up to 3 member slots (participant, in-position bit) and a state (`D+1172`);
- gathering (vf4) while it has room and the state is 0 or 1; a ped may join (vf8) when its entity type fills a free slot of the chosen row, or of any candidate row before one is chosen;
- passers-by join through the ordinary offer scan (`826C0058` -> `826BFE18`, b212): the area is an offer whose transfer expression is `ConversationIsGatheringMembers` (vf4) and `EntityAllowedToJoinConversation` (vf8) and not `IsChasing`; the founders go through the same test (`826A6CD8` ignores its result);
- add (`82E1D380` via `82E1DB50` / `82E1DC70`) turns a ped away unless it is gathering and the ped fits; each join narrows the candidate rows to the ones every member fits;
- 3 waypoints (`82E1D620`) on a 1.5 m circle 120 deg apart from a random start in [0, pi); a ped locks the nearest free one (vfunc 144 `82E1CBC0`);
- when every member signalled "in position" (vf44) it starts (`82E1EBC0`): fewer than 2 members ends it, else a candidate row is picked at random, the per-row value list gives one value, state 2, the first member speaks;
- each turn lasts the speaker's 3.0 s ConversationSpeak timer, then vf48 advances (`82E1ECB0`): state + 1, the next occupied member speaks; state 7 is complete (vf36): 5 turns. Speech and lifetime (`b43-conversation-speech-slots.md`, main spot-checked `82E3DAD8`):
- start and every advance store the turn's line id (`82E1EA98`: 2 -> 0 or 1, 3 -> 3, 4 -> 5, 5 -> 6, 6 -> 7; `D+1168`) and re-roll the variant (`D+1156`, uniform [0, 2)) once per turn;
- ConversationSpeak Begin (`826A66B8` -> `82E3DAD8`) gives the speaker the speech value line id + 33 (`ped+2468`) with the variant and the row's value (`ped+2472` / `+2476`), and every listener the value 41;
- the first join starts gathering (state 1) with a 30 s timer (`82E1D380`); when it runs out the conversation starts with whoever is there (`82E1EB10`);
- a member leaving (`82E1D470`) aborts a conversation that is not complete (vf52 `82E1E998`); the last one leaving resets it, and a spawned area is then removed (`82E1BD08`).

### Conversation waypoints

`82E1D620`: the area at `center` (already dropped onto the world by the host; `None` = the drop missed: no waypoints) with its waypoints. Each waypoint (on the circle, jittered) must land on the navmesh within the tolerance in x and z, on an area of cost at most `max_area_cost` (so never a road), and within `max_distance` of the centre; one failure clears them all (`82E1DAC0`, no retry). Its height is the navmesh's. `probe` = `None`: a map without a navmesh keeps the plain circle (NOT RETAIL: retail always has one).

## Code reference: living_world/peds/skater_contact.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/skater_contact.rs`; each comment there links its section here.

### Skater touching a ped

The skater touching a ped (doc 26, "Skater hits peds"): knock-down or standing stumble, the hit direction and the reaction animation.

Retail [code, TU3; evidence only, `.local/research/peds/skater-ped-contact.md`]: the ped contact callback `sub_82E38FB8`, kind 5 (an `IActor` owner, e.g. the skater):
- inputs: the length of the ped body's linear (f29) and angular (f28) velocity after the contact solve, gated by entity `C04236FB548697D0` / `797AA1D5F828819B` (0 for every stock entity);
- kind: knock-down when the ped's animation set allows it (byte 17, field `5B92564B352A9FAA`; off for the marquee sets) and either speed is above `541FFA2E9D81C947` / `7C5E39ECE5A5572E` (3.0 / 3.0); otherwise a standing stumble, unless the ped's brain says immune (`+3277` bit 0x10). The 6.0 thresholds (`081F978F8706D77B` / `5E566360FDC5FDD1`) and the "no reaction" cases are for a ped touching a ped (the toucher casts to `IPedestrian`, `sub_826C2C70`);
- direction: the contact normal (y dropped) against the ped's forward: `a = acos(dot)`, negated when `cross(forward, normal).y > 0`, in degrees: 1 FromBack (|a| < 35), 2 FromLeft (35..145), 3 FromRight (-145..-35), 0 FromFront (beyond 145) (constants `0x8206D148`, `0x822F9428..30`; names from the table `0x830218B0`: FromFront, FromBack, FromLeft, FromRight, Knockdown, Standing).
- reaction (`MotionGraph_Pedestrian` `motiongraph_collision.xml` / `template/knockdown.xml` [data]): see [`reaction_steps`]. The ped lies on the ground until the AI's `Recover` intent; the ground time is animation-set field `AD3C483F0C9DAD67` (1.5 s, security 0.5 s; read by `sub_8269A990` in `PedestrianColliding`'s update). Where retail starts counting it is not pinned [inferred: on the ground].

### Ped re-hit

A re-hit of a ped getting up (`template/knockdown.xml` `Falling` from `RecoveryCollision`) [data]: LyingDown plays the fall from the set's ground restart time, Crouched the crouch clip, Standing the whole fall (all blend 0.4); then the ground cycle and the get-up as for a knock-down. Returns the steps and the first step's start time. The direction is the stored one for LyingDown and the new hit's otherwise (the caller picks it, b372).

### Ped touching ped

[code] `sub_82E38FB8` kind 5 with a ped as toucher (`sub_826C2C70`): the ped thresholds; under the zombie cheat (`0x83085494` vfn156) a ped's touch never knocks down and gives no stumble either; `forced` = the toucher's chaser component answers vfn92 (`82E3D968`, its `brain+3276` bit 0x02), a knock-down whenever the set allows one. NOT RETAIL YET: what sets that bit (chaser vfn88 `82E3D920`) is not found, so callers pass false.

## Code reference: living_world/ped_hand_props.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/ped_hand_props.rs`; each comment there links its section here.

### Ped hand props

Ped hand props (b87, b89): the `livingworld_handprops` records (offset, carry channel) and their models (`living_world/hand_props.json` + `hand_props/<key>.glb`, setup `tools/asset_pipeline/hand_props.py` from the retail DMO templates), and the held object on the ped.

Retail: SpawnInteractionBasedHandProp requests the record (`82E3DDA0`, `brain+3279` 0x01); the ped update creates the DMO (`82E3DE18`) and attaches it (`82E3EC60`: `ped+5920`, `brain+3278` 0x02); every frame `82E3E4F0` / `82E3E1B0` put it at the hand matrix composed with the record's local offset [code]. NOT RETAIL YET: the hand bone is RIGHTHANDPROP (rig 26) [inferred: every carry channel is `*RH`; which bone fills the hand matrix at skeleton +19504 is not decoded]; the record fields `Hash_3FE1...` = Euler rotation in degrees and `Hash_DC20...` = translation in metres, applied translation then X, Y, Z [inferred from the value shapes; the record loader is not read].

Release (b90, b91): the brain's throw / drop (`skate_core::living_world::peds::hand_prop`) releases the object here: it becomes a prop body created mid-game (`GamePhysics::spawn_runtime_prop`, doc 27 "Props created mid-game") with the release velocity, its model follows the body, and the ped keeps the link until the prop leaves the unlink box. The ped never removes it (b90 §3); the dynamic-object pool (49) and the census cull (100 m) do.

### Hand prop proxies

The released hand props in the skater's body contact solve (b94): each one is a solid proxy with its box, mass and velocity in contact group 12 (heavy) or 14 (small), so a hit goes through the ordinary skeleton contact: the relative normal speed (halved for small objects) feeds the region forces and the usual wipeout checks (`82BD4A30` -> `82BD88A0`). Retail has no hand-prop hit code (b92 Q2). The prop's own reaction stays the prop step's skater push.

## Code reference: living_world/vehicle_contacts.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/vehicle_contacts.rs`; each comment there links its section here.

### Car ped contacts host

Traffic cars touching peds (doc 26, "Cars hit peds"; rules and retail evidence in `skate_core::living_world::peds::vehicle_contact`).

Retail [code]: a ped's contact with an `IVehicle` body is contact kind 2 (`sub_82E38400`); the ped's callback (`sub_82E38FB8`) only lets the ped's root follow its collision body, which the car's kinematic box has shoved aside. No knock-down, stumble, ragdoll, speech or fade.

Here (`FixedUpdate`, after the peds and the cars stepped this tick): every ped against every car's box (GLB bounds, the same box as the car's skater proxy), peds and cars in id order so the result is deterministic; a contact pushes the ped out ([`VehicleContactReaction::Push`], kept on its navmesh), is published as a serialisable [`VehicleContactEvent`] (the planned `sdk.living_world` events and a future host read it) and logged as `VEHICLE_CONTACT` (once per car and ped per second). Rules: [`LivingWorldSettings::ped_vehicle_contact`] (`sdk.world.set_tuning("living_world", {ped_vehicle_contact = {enabled, push}})`); a mod that stops is undone with the settings reset.

NOT RETAIL YET: the ped body is a cylinder of the NavPower agent radius / height (retail's Havok ped shape is not decoded), the push is the smallest separation in the ground plane (Havok's penetration recovery is not decoded), the navmesh stands in for the world collision of the pushed body; the car side (hit-by mask, horn, the planner stopping for peds) is V4.

## Code reference: living_world/peds/hand_prop.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/hand_prop.rs`; each comment there links its section here.

### Hand prop release

Ped hand prop release (b25, b87, b90): the throw at a trash bin, the drop, the release frame and the unlink.

Retail [code]: `sub_82E3E648` starts a throw (holding required; pending + aimed bits, target, launch speed, release time, the clip by the angle to the target), the per-ped update `sub_82E3ED50` releases the held object when the release time runs out, aimed with the launch velocity of `sub_82E16AB0` and timer 35 = the flight time, through `sub_82E3EBE0` (holding cleared, the object goes into physics); a released object still linked to the ped is unlinked by `sub_82E3FAE0` once it is outside the unlink box around the ped. The ped never destroys it (b90 §3).

The attack throw (ThrowHandPropAtWantTarget, `sub_82E3E960`, b92 / b93) aims at the target first: both the target and the ped are predicted ahead, the XZ intercept solve `sub_82E15CD0` picks the earliest point (or, with no solution, the ped's own predicted position), a random horizontal jitter and a lift follow. A released prop that hits the skater is an ordinary prop contact: retail has no hand-prop hit, bail, speech or score code (b92 Q2).

### Hand prop intercept

The intercept solve `sub_82E15CD0` [code, b93 B2 / B3]: a projectile of flat speed `speed` from `origin` meeting a target at `aim` moving at `velocity` (XZ only). Solved along the dominant axis (strict: equal |dx| and |dz| have no solution); returns the target's point at the earliest non-negative time (Y of `aim`), `None` with no solution. Ported as read, including the `v_minor + k * v_major` term (an exact intercept would subtract; b93 B2).

## Code reference: living_world/peds/takedown.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/takedown.rs`; each comment there links its section here.

### Ped takedown choice

Ped takedowns (doc 26 "Ped takedowns"): which takedown a chasing ped can try on its target. Retail (TU3, evidence only; re-implemented; `.local/research/peds/b16-takedown-choice-contact.md`, main checked the setup data): the ped type's `livingworld_entity_takedown` record lists takedowns in priority order. Each tick (`82E3C000`, from TakeDownTargetablePredictions):
- the approach side: `n = normalize(ped - target)`, `dot = n . target velocity`; side 1 when the target comes towards the ped (`dot > 0`), else 2; `speed = |dot|`;
- an entry with a side needs that side and a speed above its minimum;
- per entry (`82E3B548`): the target `lead + 0.06` s ahead (`pos + vel t`); more than 1.25 m above or below fails, else flattened to the ped's height; the distance from the ped's reach point (`ped + 0.2 x forward`) must lie in `(min + 0.2 + 0.06 s, reach + 0.2 + 0.06 s]` (`s` = the ped's speed); the signed bearing (about up, positive when `(forward x to_target) . up > 0`) in `[angle_min, angle_max]` degrees, or, for an entry that allows it, in `[-angle_max, -angle_min]` (mirrored, the `anim_b` clip);
- the first entry that fits wins.

## Code reference: living_world/peds/perception.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/perception.rs`; each comment there links its section here.

### Ped perception

What a ped knows about and sees (doc 26 "Ped perception"). Retail (TU3, evidence only; re-implemented; `.local/research/peds/b17-chase-rest-lost.md`, `b18-ped-perception-mood-ops.md`, main checked the setup data):
- the brain keeps at most 5 perception entries (`brain+624`, 80 bytes each); a new entry (`82E42690`) evicts the oldest (largest age), starts with "just told" (flags 0x80), the given memory, age 0 and no mood suppression;
- each tick (`82E418E8`) memory counts down, age up, the suppression down; the vision test sets "visible" (0x40) and refreshes the last position and velocity while seen; "just told" is cleared; an entry whose target is gone, or whose memory ran out while not just told, is erased;
- the vision test (`82E27240`): seen within 1 m on the ground plane and 2 m in height (touch), or within the near range from the eye, or within the far range inside the view cone (`dot(dir, forward) > |cos(fov)|`) with a clear line of sight. Near / far = the mood event category `chasedpersondetectibilitytest` (20 / 30 m) times the ped type's perception scales (`D036` / `D238`, pedestrians 1.0); fov = `7435` (120 deg).

## Code reference: living_world/peds/flee.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/flee.rs`; each comment there links its section here.

### Ped flee

Where a fleeing pedestrian runs (doc 26 "Ped behaviour runtime"). Retail (TU3, evidence only; re-implemented; `.local/research/peds/b8-ped-flee-steering.md`, main checked the leg length, the speed gate and the blend):
- the Flee intent installs a goal provider (`sub_82E35E80`, provider vtable `0x8232C818`) and an arrival radius of 2.0 m (`S.vfn136`, `0x82060C50`);
- the direction (`sub_82E318F8`): `away` = flat ped position - flat threat position; when the threat moves slower than 1 m/s (`0x8231A844`) or the ped is behind its motion, straight away; else `0.5 x (side + normalize(away))` (`0x8209975C`; not renormalised), `side` = the threat's motion direction x up, on the ped's side;
- the goal = position + direction x 15 m (`0x820BD16C`), then a navmesh cast (`sub_82C465A8`; a blocked goal is rewritten by `sub_82C46208`: we stop at the last clear point, inferred);
- a new leg when the ped is within the arrival radius of its goal (`sub_82E2BC70`, 3D). The run speed is the ped type's chase record (`CD65`, 11 for pedestrians) turned into a gait (`sub_82E2D260`); our body plays the run cycle and moves by its root motion.

### Object flee

A ped flees a prop moving at it (ask 374, 2026-10-26). Retail (TU3, evidence only; re-implemented; main read `82E2EDC0`, `82E2F018` and `82E2EF40` in Ghidra and the generated code, `.local/research/b646/`):
- the gate (`82E2EDC0`): the flat distance from the ped to the prop's position squared is at most 64.0 (8 m, `0x820ED958`) and the prop's velocity squared at least 0.5625 (0.75 m/s, `0x822F915C`);
- the lane (`82E2F018`): side = the prop's flat motion direction x up; the prop's four footprint corners (plugin object `0x8269B3D0` vt+0x94) give the lane's two edges along side (each at least 0, the prop's centre); the ped's offset along side must lie strictly inside the lane widened by 0.75 m (`0x821814A0`) each way; then the ped must be ahead of the edge corner on its side: (ped - corner) . velocity > 0, flat;
- the write (`82E2EF40`): all 17 want slots lose "needs addressing" (`& 0x7F`), then the flee slot (`brain+0x214`) gets the prop's handle and flags 0x80 (needs addressing, 0x40 cleared). No mood event, no roll: the ped graph's Flee state (`pedestrian_wanttoflee.xml`) takes it from there.
- a sibling for vehicles (`82E2C0C8`, IVehicle `0x82C34050`, 10 m, moving at the ped) is not part of this change.

Ours: `skate_core::living_world::peds::object_flee::in_path` (the gate and lane, `ObjectFleeParams` with the retail values), `PedBrain::flee_object` (the write), `think_peds` tests every streamed-in prop per ped and tick (props in id order; prop want targets are `PROP_TARGET_BASE | body id`, top byte 0xFE, so a prop resolves as a flee target and the Flee state's `HasEscaped` distance works), log `PED_FLEE_OBJECT ped=#N prop=id at [..]`. The flee legs now read the threat's velocity for any target (peds, players, props), not only players. Engine choices: the footprint corners are the box's x / z faces (retail's corner source for a tipped prop is not read); how often retail runs the gate is not proven (ours: every think tick). Tuning (mods): `ped_brain.object_flee`, `object_flee_radius`, `object_flee_min_speed`, `object_flee_lane_margin` (0..=100; retail on, 8.0, 0.75, 0.75). Multiplayer: decided on the ped authority from positions and velocities, no random rolls. Tests: skate-core `object_flee` (2), `brain` (1), skate-mods `world_tuning`.

## Code reference: living_world/ped_plugins.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/ped_plugins.rs`; each comment there links its section here.

### Ped plugin data

Ped plugin data (doc 26g "Ped plugins on world props"): the prop classes (`livingworld_props`: descriptor, hand props, numeric fields kept by hash until decoded), their descriptors (`PluginDescriptor/<name>.stategraph`: graph, max participants, transfer expression), each ped type's `plugin_odds` (`livingworld_entities`, parents merged) and the district's placed plugin props (`waypoints.json`, from the RW waypoint groups). The prop side and the rolls are `skate_core::living_world::peds::plugins`. DMO hotpoint seats (benches) need the hotpoint export (open).

### Hotpoint follow prop

The map's DMO hotpoint plugin props (`native-props/<map>.json` `plugin_props`, setup `hotpoint_data`): one one-waypoint prop per hotpoint of each placed bench, bin or newspaper box (`82C4E128`), at the initial placement. The id is the DMO instance id with the hotpoint index in its top byte (stable for a host). The hotpoints belong to the object, so a moved prop carries its seats (`follow_prop`, applied every think from the body's pose): a hotpoint authored at `position` / `facing` on a prop authored at `spawn` (origin, basis columns), carried to the prop's pose `now`, keeps its place on the object.

## Code reference: living_world/peds/plugin_motion.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/plugin_motion.rs`; each comment there links its section here.

### Plugin motion states

The ped motion graph's plugin states (`MotionGraph_Pedestrian.xml` includes `motiongraph_sit.xml`, `motiongraph_atm.xml`, `motiongraph_vend.xml`, `motiongraph_waterfountain.xml`, `motiongraph_newspaperbox.xml`) [data, b84 / b85]: entered on the monitored packet's name, each state plays its logical clips in order (`PlayRemappedAnimation`, blend 0.1, the next on `WillExpire 0.01`); a looping clip holds until the packet's next stage intent appears (`StandUp`, `FinishDrinking`); the last step ends the packet (`MajorIntentComplete`, after `IntentStageComplete decrement` on the machines). The clip behind each logical name is the ped's anim set remap.

NOT RETAIL YET: a step table in place of the motion graph itself (the full graph is a later milestone); the sit-avoid states and the Collision exits are not ported. The hand prop the vending machine and the newspaper box spawn (`SpawnInteractionBasedHandProp` in the collect clip's branch window, `InTurnBranchWindow`) is the step's `hand_prop` flag; the host requests it when the clip enters its branch window.

## Code reference: living_world/peds/honk.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/honk.rs`; each comment there links its section here.

### Run from honker

Running from a honking car (doc 26g "Peds run from honking cars"). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b44-horn-honker.md`, `.local/research/peds/b45-runfromhonker-flee-timeout.md`; main re-read the honk receiver `sub_82E3C3D0` and the shape of `826A1358`):
- a traffic car's horn kind 2 writes its id into the ped brain's honker (`+3232`); `IsBeingHonkedAt` leads Wander / WanderFollow into RunFromHonker; only Wander's Begin clears the honker;
- RunFromHonker Begin (`826A1330`) posts motion intent 5; Update (`826A1358`) looks the car up every frame and, while it exists, sends the ped to a goal `side_distance` sideways of the car's line, on the ped's side (strict `dot > 0` test, a tie goes to the minus side), at `run_speed` when the goal is more than `near_radius` away, else `near_speed`. A car that is gone leaves the last goal and speed; the op's `timeout` (30) is never read. The car vector Update reads (`[car+164]+144`) is not settled: velocity or an axis (open, b45); ours uses the car's forward axis, so a standing car still gives a side.

## Code reference: living_world/ped_alerts.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/ped_alerts.rs`; each comment there links its section here.

### Chase alerts

The alert over a ped that warns (yellow) or chases (red) the player: retail's "ChaseArrow" HUD (b191, doc 26g).

Engine-facing: `PedAlerts` (settings with retail defaults, the list, the textures). The states come from the peds' `SendChaseStateMessage` (`PedEvent::Chase` kinds `state_*`, player targets only). Mod-facing: `sdk.ui.chase_alerts(enabled)` hides or shows them (released when the mod stops); mods also see the same `ped_chase` events. Textures: retail's `arrow_warn` / `arrow_chase` / `arrow_tired` / `arrow_giveup` (`private/hud/chase-arrows`, setup's `prepare_chase_arrows.py`).

Placement over the head: retail anchors on the translation of the ped matrix its vfn 0x28 returns (not decoded, b191 open 1); the alert shows over the ped's head in retail, so ours anchors `anchor_height` m above the ped's root.

## Code reference: living_world/ped_mood.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/ped_mood.rs`; each comment there links its section here.

### Mood table loading

The stock mood tables from setup data (`private/living_world/tables.json`, classes `livingworld_moodeventcategories`, `livingworld_entities_moodreactions`, `livingworld_entities_moodresults`, `livingworld_entities`) into `skate_core::living_world::peds::mood::MoodTables`, parents resolved (doc 26 "Ped behaviour runtime"). Field bindings: `.local/research/peds/b6-ped-mood-fields.md`.

## Code reference: living_world/ped_graph.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/ped_graph.rs`; each comment there links its section here.

### Ped AI graph loading

The stock pedestrian AI graph (`Pedestrian.stategraph`) loaded on the shared graph runtime, with each operation parsed into the ped brain's `PedOp` (doc 26 "Ped behaviour runtime"). Retail runs this graph on the same dynamic controller as the skater's graphs, and the shipped file is already compiled (includes and templates expanded), so the existing decoder, binder and compiler load it unchanged (`.local/research/peds/b2-ped-aigraph-interpreter.md`).

Moddability: the graph path is data (`PedGraphPaths`); operation values come from the graph's attributes. Not-yet-ported operations are listed by [`PedGraph::pending`] and logged once.

## Code reference: living_world/peds/chase_alert.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/chase_alert.rs`; each comment there links its section here.

### ChaseArrow logic

The alert over a ped that warns or chases the player: retail's "ChaseArrow" HUD (b191).

The graph op `SendChaseStateMessage` (`826A77A8`) posts {ped, state} for a player target; the HUD's handler (`825E35F0`) keeps at most 10 entries: a new ped is added (not for "end"), a known ped's state is replaced and its timer restarts, "end" removes it. The update (`825E2738`) ages them (warn 6 s, giveup 3 s, chase and tired stay), drops a ped that is gone, projects the ped and scales the sprite by camera distance. The render (`825E2B80`) draws a 64 px sprite per state over the ped when it is inside the centred 0.85 box, else an edge arrow on a ring round the screen centre. Engine-independent: the host feeds states and positions and draws [`Placement`]s.

## Code reference: physics/solid_contacts.rs

- `constrained_position` (the walker's capsule sweep): the solids are the network proxies (mod solids) except the ped
  parts (group 11): retail's walker queries never include the PedestrianPool (b266), only the skeleton meets a ped
  (laptop recomp sessions b271: the on-foot player passes a ped at up to ~6 m/s). `blocked` gets each solid that pushed
  the capsule out or stopped the sweep, with the contact normal (toward the walker).

## Approach with no path

(2026-10-22, research b515 / b516; found while looking into USER-ASKS 141 i)

- **Retail [code, b515 / b516]:** ApproachWantTarget (`8269FDF0`) hands the target to the ped's mover, which ends in
  the NavPower bot path request (`82E957F8`). The path query (`82928BB0`, A* `82932CB8`) builds a path only when it
  reaches the goal poly: no partial paths. On a failure `82E96F08` puts the bot in mode 0 with an idle follower at
  the ped's spot, so the ped stands. In mode 0 the skip test (`82E95668`, a 3D distance with `fsqrts`, main-checked)
  compares with the ped's position, so the query runs again every update while the target is out of reach.
- **Ours before:** the approach set a one-point route; with no path `PedNav::plan` walked straight at the point
  (`vec![target]`, the wander targets' retail fallback) and pushed against the navmesh edge.
- **Change:** `PedNav::set_approach_route` (used by the ApproachWantTarget branch in `living_world/peds.rs`) marks the
  route; a failed plan for it sets `holding` and the ped stands (Idle), planning again every update; any path found
  clears it. `set_route` (mod routes, the other brain routes) keeps the straight fallback.
- **Not ported:** retail's re-request thresholds while a path is followed (0.13 m raw / 0.325 m snapped = 0.1 / 0.25
  x the NavPower scale 1.3; b516): ours re-plans the approach every tick, which only costs queries. The arrival
  radius term from the bot settings, and keeping the old path for a target off the mesh, stay open (b516).
- **Tests:** `an_approach_goal_with_no_path_leaves_the_ped_standing` (skate-core nav tests). Not play-tested.

## Chase reach check snaps like retail (2026-10-28, USER-ASKS 392, research b650, b653)
- Problem (user, 2026-10-27): "the ped is also NOT giving up on chasing me even though he CANNOT reach me".
- Retail [code, b650]: there is no stall or path-failure give-up for a chaser. Near the chasee (5 m) the only exit is
  CannotReachChaseTarget, set by the mover's reach check (`82C465A8`) every 10 updates. That check snaps the ped and
  the destination with `82C46348` [code + data, b653]: the 3D-nearest walkable polygon point with no distance cap and
  NO height test, failing (code 3) when it lies more than 0.65 m away along X or along Z (radius 0.5 at `0x8209975C`
  x the NavPower params scale 1.3 at `0x82099484`; main checked the compare at skate3_recomp.78.cpp:17500).
- Ours used `NavMesh::locate` (0.5 m round edge radius, 4.0 m height window) for the check.
- Change: `NavMesh::reach_snap` and `chase::reach_failed_at` (skate-core `peds/nav.rs`, `peds/chase.rs`); the intercept
  and the reach check in `living_world/peds.rs` use them. Mod key `living_world.ped_brain.reach_snap_window`
  (retail 0.65). Test: `reach_reports_fail_off_the_mesh_and_across_islands_and_only_they_rewrite_the_bit`.
- What it means in play: a chasee standing inside a prop footprint cut from the navmesh, or more than 0.65 m off the
  walkable area, reads unreachable within 5 m (wait 5 s, give up). A chasee on a ledge or rail over walkable ground
  is reachable in retail too. Why ped #189 stood still on a route the check called reachable is a follower issue,
  still open (needs a route log in a session). Open: the polygon class test `82C47D60` (poly +0x2C nibbles >= 2),
  the in-polygon projection along the normal on slopes.

## Scatter alarm (2026-10-28, USER-ASKS 381, research b654)
- Retail [code + data, b654; main checked the writer and the reset in the recomp]: `sub_826C1348`, once per world
  step, reads the local player's PhysOut_Trick copy of the MotionGraph gesture block. The alarm byte (MG +5989) is
  written by `sub_8258FB30` (stw +3300 / stb +3305 / stb 1 +3304 on MG+0xA7C) from CharacterGesture (`82BAA320`) as
  "the running gesture is on D-pad Down" (k == 3); the reset `sub_825960C0` sets the id back to 37 (no gesture) and
  clears both bytes. While the byte is set the timer is SET to 180.0 (`0x821DE7BC`), then 1/60 comes off per step;
  scatter while above 0. Zombie mode: off, timer untouched. Every live ped's ShouldScatter (brain +3281 bit 0x80) is
  that result (`sub_826BDB50`); the graph's Scatter state speaks value 20.
- Ours: nothing wrote `b.scatter`, so Scatter never ran.
- Change: skate-core `peds/scatter.rs` `step`; `think_peds` steps it once per world tick from the player's
  `gesture_publication.down` (our CharacterGesture port, `graph_host/motion_character_gesture.rs`) and copies it into
  every ped's brain. Log `PED_SCATTER on|off`. Mod key `living_world.ped_brain.scatter_seconds` (retail 180, 0 = off). Since 2026-10-29 also `living_world.ped_brain.scatter_hold` (ours): while true the alarm is raised every world step like the held gesture; set back to false, retail's timer runs out (skate-mods 107, skate-game tuning / ped 127 passed).
  Test `the_alarm_holds_while_raised_runs_180_s_and_is_off_in_zombie_mode`.
- Inference: the byte clears when the gesture ends (our publication is None then), read from the reset setting id 37;
  the reset's caller is a table entry, not traced. Open: a Lua raise / clear call; one recomp check (SCATTEST hook,
  a D-pad Down gesture near peds) would confirm the byte. User (2026-10-28), asked whether the D-pad Down gesture near
  peds makes them run off in retail: "Yes that sounds correct." Multiplayer: the host owns the timer; with several players
  retail reads only the local player (online is open). Not play-checked.

## Vehicle flee opt-in (2026-10-29, W8, research b656)
- Retail [code, `82E2C0C8`, the ped's VehicleAvoidance modifier; b646 pseudocode, generated code checked by main]: a car
  (IVehicle `0x82C34050`) within 10 m (100.0 squared at 0x820ED57C, 3D) makes the ped flee when the flat dot of
  (car - ped) and the car's velocity is above 3.0 (0x82063B08): read literally, the car moving AWAY along the line.
  Then every want's active bit is cleared and the flee want takes the car (same write as the prop rule).
- Direction (USER-ASKS 401, 2026-10-29): flee both ways, as an opt-in for testing.
- Change: skate-core `object_flee::VehicleFleeParams` / `flees_vehicle` (either direction past the dot); opt-in "Ped
  Vehicle Flee" (overlay Opt-Ins, `SKATE_PED_VEHICLE_FLEE=1`, off by default) sets `ped_brain.vehicle_flee.enabled`;
  `think_peds` raises the flee want after the prop rule (log `PED_FLEE_VEHICLE`); cars are want targets (entity type
  and velocity) so the flee leg resolves. Mod keys `ped_brain.vehicle_flee`, `vehicle_flee_radius`,
  `vehicle_flee_min_dot`, `vehicle_flee_away`, `vehicle_flee_toward`. Test
  `a_car_within_ten_metres_moving_along_the_line_either_way_makes_the_ped_flee`. Not play-checked.
- Open: retail's gates IVehicle vf+0x60 and the ped's vf+200 (not read); the non-flee branch (a ray test of the car's
  4 corners from 1.5 m up along the ped's heading, 4.0 x lookahead, 0.4 / 0.09 thresholds, "Avoid" event) is decoded
  in b656 but its consumer is not found, so the steer-around is not ported.

## Greet and warn gestures, StopChannel on collision (2026-10-29, research b677)

**Problem.** Peds said their greet and warn lines without the matching upper-body gesture, and a gesture or hand-prop
clip kept playing through a knock-down.

**Retail [code / data, b677].**
- `ChannelWarnWantTarget` Begin `8269F7B0` and `ChannelGreetWantTarget` Begin `8269FAC0`, only when the want target
  resolves: the channel request ped vfn+0xF8 `82E3A3F0` with "WarnChannel" / "GreetChannel" (strings `0x82307D04` /
  `0x82307D1C`, blends 0.2 / 0.2 `0x82099280`), then the 3.5 s timer, then the speech. Without a target the warn says
  nothing and drops the want when `deactivateWant` is set. The anim set fields: `GreetChannel` `Hash_6F928ECB7DFA7A05`
  (`NPC_GRTCHN_POS_LO_0_01..05`, marquee `IP_GRTCHN_*`), `WarnChannel` `Hash_C3298FF93E24D541` (`NPC_WARN_CHN_*`).
- The Collision state machine runs `StopChannel` (`motiongraph_collision.xml`).

**Change.**
- skate-core `peds/anim.rs`: `GreetChannel`, `WarnChannel` in the loaded names; `channel_stop_own` (stop with the
  clip's own blend-out; the behaviour's fade is NOT RETAIL YET).
- `peds/brain.rs`: both Begins push `ChaseRequest::GestureClip` when the target resolves; the warn Begin also gates
  its timer and speech on the target and drops the want without one; `BrainSettings::gesture_blend` = 0.2 (data).
- `peds.rs`: the host plays the gesture on the channel (`PED_GESTURE` log) and stops the channel when a collision
  reaction starts (deferred or direct).

**Verification.** skate-core living_world 402, skate-game ped 95, skate-data ped anim 5 passed; brain tests updated
(warn without a target, greet / return greet gestures). Not seen in game.

**Open.** PlayerRecognition (`826A1188`) and ProTrackSkater (`826A2C80`) also post GreetChannel while the channel is
idle (ops still pending); how retail picks within a field's list (ours uniform).

## ATM and water fountain plugins: retail places none (2026-10-29, research b678)

The classes ship (`livingworld_props` `waypoint_atm`: `useatm.xml`, 0.8, radius 15; `waypoint_waterfountain`:
`usewaterfountain.xml`, 0.6, radius 15) with their plugin graphs and motion graphs, but neither placement source holds
one [data]: the world waypoint groups (`WAYPOINTDATA` 0x00EB001A, `waypoints.json`: 74 groups, all vending machines or
trash bins) and the DMO hotpoints (`82C4E128` makes only trash bin, newspaper box or seat [code]); no map DMO is an ATM
or a drinking fountain. So retail peds never use them, and ours match (our loaders take any class the data names,
`ped_plugins.rs`). Possible mod work only: a content-layer / Lua way to place plugin props per map.

## Chase lines: the caught line at a group end, no chase-over line (2026-10-29, research b680)

**Retail [code].** Three graph ops post the chase line (chase manager slot +64, `826B4FC0`), all with first argument
1: TakeDownTargetableSuccess `826A4E50` (1, 0) a CAUGHT line for a player target; ChaserGroupEndChase `826A5318`
(1, 0) a CAUGHT line when the chaser is in a group, the op's reason is aggressivecapture and the chasee is the local
player (this runs when a chaser gets close enough, or when the chased player wipes out); the taze op `826A8420`
(1, 1) a TAZER line once per hit. `ID_CHASE_OVER` (first argument 0) is never posted in TU3: escapes and give-ups show
no line.

**Change.** `ChaseRequest::GroupEnd` carries `notify` (true from the op, false from the taze path's group end, which
retail ends through group vfn24 without a line); the host emits `PedEvent::ChaseCaught` under retail's gate and
`post_chase_messages` shows a CAUGHT line for it. Test `a_chase_caught_event_posts_a_caught_line`; brain tests
updated. The line roll is now seeded once per run from the clock (a fixed seed made most takedown lines in the logs
CAUGHT_2; presentation only). Not seen in game. Open: a data table for the chase message ids and bands (mods).
