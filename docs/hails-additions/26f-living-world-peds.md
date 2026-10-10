# 26f: Living world: Pedestrians: body, navigation and look

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Change: peds milestone M2, the ped body

Pedestrian spawn records become visible, animated peds with footstep audio.

What retail does, checked in the code and data for this milestone (TU3):
- The entity inside the rolled category: `sub_826B8B88` draws `sub_826BB058` (world RNG u32 x 2^-32 as f32,
  `0x822F88F4`) and takes `trunc(draw x 100) % count` (`0x820ED57C` = 100) of the category's `entities` array
  (`sub_8269B040`); an empty list spawns nothing [code]. The model's tint pair comes from one rand `r`:
  `tints_a[r % na]`, `tints_b[r % nb]` (`sub_827B4170`) [code].
- `PedestrianSkeletonPres.abin`: 462 VBR clips, a 50-bone rig with trajectory and 10 parts; the clips carry the
  first 6 parts (bones 0..=26), additive over the `PEDESTRIAN_RIG_TPOSE` pose record; fingers, face and twist
  helpers have no clip data [data]. Clip attributes `LEFTTOEDOWN` / `RIGHTTOEDOWN` / `LEFTHEELDOWN` /
  `RIGHTHEELDOWN` (phase windows) and `BODYFALLTYPE` (values) are the foot plants and body falls the ped audio
  reads [data]. The walk clip's trajectory moves 1.325 m/s (recomp walking median 1.30).
- `livingworld_entity_animation` names its clips by vault hashes of the motion graph's logical names
  (`FwdWalkCyc` = `Hash_4AA12E0083F10739`); `tAnimAttributes.anim` is a byte offset into
  `skatercollections.bin`'s string pool [data]. The motion graph gives the blends and exits: idle 0.1 (cycle
  swap 0.5), Stand2Walk 0.25 (exit 0.03 s before its end), walk 0.1, Walk2Stand and turns 0.15 (exit 0.1 s
  before the end; Walk2Stand inside a walk branch window), left turns mirror the right clip [data].
- The 51 ped GLBs' 39 joints are all rig bones by name; their bind skeletons match the animation reference to
  1 cm [data].

Change:
- Setup: `living_world_anim.py` adds `anim_name` / `anim_b_name` beside those offsets (tables otherwise
  identical; only the `livingworld` fingerprint changes).
- `skate-core::living_world::peds`: `choice` (catalog, the retail entity index, tints, overrides, `PedLook`),
  `anim` (clips, rig, the player: remap, blend, mirror, root motion, foot channels, the locomotion states,
  `TestPath`), `match_bones`.
- `skate-data::ped_anim`: `PedBank` (rig, reference pose, partial-part clip decode with attributes),
  `PedTables` (categories, entities, models, animation sets).
- `skate-game::living_world::peds`: one entity per pedestrian record (`Pedestrian`, `PedBody`, `PedAudio`), the
  GLB bound to the rig by name, bones without clip data following their parent with the bind offset, ground snap,
  root motion, `PedAudio.feet_down` / `body_fall` / voice, `PedLooks` overrides, `PedEvent`, debug readout with
  `SKATE_LIVING_WORLD_DEBUG=1`.

Simplifications until later milestones (documented, not retail): no navigation (M3): a ped idles, walks a few
metres straight, stops and turns round (`TestPath`); the motion graph is not run (M4): the locomotion subset is
hand-wired from its data, the crossfade is linear over `blendTime`, the idle cycle swap is a uniform draw per
wrap; LOD switches LOD0 / LOD1 at the model's 45 / 55 m pair (meaning unconfirmed, `sub_827C1188` not read),
animation runs every tick for every ped; tints are kept but not drawn (the shader mask is not decoded); the group
model child is a seeded uniform pick (code not found); the `granny` set names `GRAN_WNDR_*` clips no shipped
bank holds, such peds use the `default` set.

Files: `crates/skate-core/src/living_world/peds/{mod,choice,anim,tests}.rs`, `crates/skate-data/src/ped_anim.rs`,
`crates/skate-data/tests/ped_anim_data.rs`, `crates/skate-game/src/living_world/{peds,peds_tests}.rs`,
`tools/asset_pipeline/{living_world_anim,test_living_world_anim}.py`, registration lines in the three
`mod.rs` / `lib.rs`, `living_world.py`, `versions.py`.

Verification (peds M2):
- `cargo test -p skate-core --release --locked living_world::peds`: 11 tests (retail index formula, tints,
  seeded looks, overrides, bone matching, idle / start / walk / stop, walk speed at 30 / 60 / 144 Hz, mirrored
  turns, determinism, reference add, idle swap, test path).
- `SKATE3_ASSET_ROOT=<roots> cargo test -p skate-data --release --locked --test ped_anim_data`: every clip
  decodes, rig and reference, walk 1.325 m/s, turn -3.07 rad, every census entity resolves a look and its
  clips, all 51 GLBs match the rig (bind vs reference 1 cm).
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world::peds_tests`: 5 tests (seeded
  looks, despawn, rejection, state = f(record, tick) at 60 / 144 Hz, foot plants, overrides, LOD, followers,
  data-gated load).
- Headless render: a mid-walk pose skinned onto `male_jock_2` (`render_glb.py --pose`) shows a correct stride.

Multiplayer: the look is a function of the spawn record; the body steps once per world tick from the spawn
tick; a client rebuilds the same ped from the same record. Moddability: `PedLooks` (category entity lists,
entity model / animation set, recipe GLB), `PedEvent`; restoring `PedLooks::default()` undoes a mod for new
spawns. `sdk.living_world` ped calls come with the mod milestone.

Plan line for doc 26 (milestone table): "peds M2 ped body: done (looks, animation player, foot plants; TestPath
until M3)". Open-questions additions: the 4 parked items below.

## Change: peds milestone M3, navigation

**What retail does** (TU3 code read; addresses are evidence only):
- **Ambient peds never use the road branch.** `Pedestrian.xml`'s `Wander` takes `WanderMode.Road` (FollowRoad +
  `UseCrossWalk`) only while `HasRoadWanderTarget`, and `WanderOnRoad` on `IsOnRoad`; in
  `StateGraph::TheConditionFactory` (`sub_826C1730`) both are registered with the generic factory `sub_82BC3F68`
  (`sub_82F7A698`, `sub_82F79698`) whose evaluate (vtable `0x8231ED5C` + 48) returns 0 [code]. Every ped runs
  `WanderMode.NoRoad`: `CheckForRoadTarget` (no-ops) and `NoRoadWander` (`sub_826A2FB8`). The crosswalk states and
  `WalkSignSaysGo` (`sub_826AC190`) are unreachable for ambient peds; the working `IsOnRoad` / `IsNearCrossWalk`
  belong to the plugin transfer conditions (`usetrashbin.xml`).
- `NoRoadWander` puts the ped on its NavPower mover (`ped+2224`, vtable `0x8232C708`) with the wander goal
  (`ped+5632`, vtable `0x8232C9C8`, mover value 2.0). Target (`sub_82E30F58`): from the ped's forward, probe 5
  directions within 90 degrees at 40 m, else 9 within 270 degrees at 10 m (skipped to the second fan after a
  NavPower failure event 1-3), else step `max(3, 1.1 x 2.0)` m ahead. Probe order (`sub_82E311A8`): straight, then
  `ceil(i/2) x (spread/2)/(n/2)`, negative first. A direction fits when its end point snaps to the navmesh (0.5 m)
  and is reachable (`sub_82C464F0`, a connectivity bitset); NavPower plans the path and steers. On arrival (bot
  state 1, `sub_82E2CB08`) the goal picks the next target.
- **Navmesh** (`0x00EB0027`, all 678 objects) [data]: NavPower v23 graphs per tile: agent block 0.12 / 0.35 / 0.2 /
  1.6 (cell, radius, step, height; unverified), tile bounds, polygons (centroid, radius, flags; the area byte in
  bits 8-15 of the second flags word) with 24-byte edge records (neighbour offset relative to the graph base,
  vertex, edge flags). 63283 polygons, every one of the 148258 neighbour links checked. Areas: 0x11 pavement
  (91-92 % of recomp ped positions), 0xA1 road carriageway (2054 of 2147 DownTown road piece starts; 3-4 % of ped
  positions), 0xF1 never stood on [data + trace]. Tiles stop 0.14 m short of their borders; cross-tile neighbours
  are resolved at setup.

**Change.**
- Setup (`livingworld` group): `living_world_navmesh.py` writes `living_world/navmesh.bin` (polygons, areas,
  neighbours incl. cross-tile links) and `navmesh.json` (counts, area histogram).
- `skate-core::living_world::peds`: `nav` (`NavMesh`: point location with the 0.5 m snap, reachability components,
  A* + funnel paths; `NavRules`), `wander` (the retail target choice, `PedNav` per ped, avoidance, step checks,
  `CrosswalkRule`), `crosswalk` (walk lights from the shared `SignalClock` for the mod rule).
- `skate-data::ped_nav`: `navmesh.bin` reader / writer.
- `skate-game::living_world::peds`: the district navmesh in `PedData`, `PedNavSettings`, navigation-driven intents,
  every step kept on walkable polygons and 0.7 m from other peds; maps without a navmesh keep the test path.

**Simplifications (not retail, documented):** NavPower's path search, path following and local avoidance are not
decoded: paths are A* over polygon portals (equal cost) with funnel corners, a ped turns at 45 degrees per second
while walking (locomotion field `Hash_AD1EA18F819BF397` = 45 read as degrees per second, unverified) and pivots
when a corner is more than 60 degrees off; avoidance = yield to a lower id ahead, side-step a higher id, never
closer than twice the agent radius; 0xF1 polygons blocked (trace-backed); event 4 (`+33`, head back) has no
trigger yet; spawn points snap onto the navmesh (retail's spawn probe not read).

**Moddability.** `PedNavSettings` (wander fans / distances / fallback, turn and avoidance values, `NavRules`
blocked areas and area costs, crosswalk rule), `PedNav::set_route` (mod routes), `NavMeshInput` (a custom map's
walk areas, or `navmesh.bin` written with `skate_data::ped_nav::write`), `WalkSignals` (own crossing lights). The
crosswalk rule (`WalkSignal`: wait while the walk light is not green) is the unused retail `UseCrossWalk` logic,
off by default.

**Multiplayer readiness.** A ped's walk is a function of its spawn record, the world tick, the navmesh and the
other peds' positions (stepped in id order); no hash-map iteration, ties broken by polygon index.

**Tests.** skate-core 8 (fan order, location / reachability, paths, target choice, wander on walkable ground +
determinism, crosswalk waits for walk, clock mapping, avoidance radius); skate-data 1 + 3 data-gated (DownTown
counts and areas, road pieces on 0xA1, 15 peds x 90 s never off walkable ground and identical on rerun, crosswalk
rule: every road entry at a signalled arm on walk green); skate-game 2 (navmesh wander in the app, export load);
Python 4.

Credits: NavPower v23 constants cross-checked against DumbadsSkate3ModdingTools by Ethanw05 (credits there to
SunJay, Dumbad, RenderWareGavin, Tuukkas); recomp: skate3recomp / rexglue / Xenia (code reading and PEDXYZ traces).

Plan line for doc 26: "peds M3 navigation: done (NavPower navmesh decoded, retail NoRoad wander, avoidance;
crosswalk rule as mod option since retail never uses it)". Open-questions additions: items 1-5 below.

Player memory agrees with the code (user, 2026-10-05): "I don't remember them using crosswalks in the retail game".

## Peds standing on their heads in the left turn, 2026-10-05

**Problem.** First play of the living world: the pedestrians "look like they hurt" (user). Twisted bodies.

**Root cause.** The left turn plays `StandTurnR180` mirrored. `PedAnimPlayer::pose` mirrored the bare clip
delta (before adding the `PEDESTRIAN_RIG_TPOSE` reference) with `pose_mirror::mirror` in trajectory mode 1. That
mode is a full-pose operation: for the children of the trajectory bone it multiplies in the literal 180 degree
quaternion (`0x8232F740`, part of `Mirror` `0x828CDAF8` [code]) that the rig's reference hips carry. On a delta
(near identity) it rotated the hips 180 degrees, so every ped stood on its head for the whole mirrored turn and
through the blends into and out of it. Crossfade blending was not the cause: `pose_blend::blend_sample` already
takes the shorter arc (dot < 0 check).

**Evidence.** The extended dump test `living_world_ped_pose_dump_for_a_render_check` writes 15 poses per set
(mid-blend into start / walk / stop / idle / both turns, mid walk, mid stop, idle, mid turns) for the `default`,
`female` and `jock` sets; rendered on `male_adult_1`, `female_adult_1` and `male_jock_2` with `render_glb.py
--pose`. Before: walk, blends, idle and the right turn are upright, all mirrored poses are upside down or lying
flat on all three models. The reference pose against the mirror on the export [data]: mode 0 leaves the hips 180
degrees off, mode 1 keeps every bone within 6.6 degrees (the authored hand asymmetry), so mode 1 belongs on full
poses. After: the mirrored turn is the upright mirror image of the right turn on all three models.

**Change.** Each layer becomes a full pose (delta added onto the reference) before it is mirrored, then the
layers blend. Blending full poses equals blending deltas and adding afterwards (the add is a left multiply /
affine map, which nlerp and lerp commute with), so unmirrored output is unchanged. No new values; nothing a mod
reaches changed (sets, clips and overrides as before).

**Files.** `crates/skate-core/src/living_world/peds/anim.rs` (`pose`, new `add_reference`),
`crates/skate-core/src/living_world/peds/tests.rs` (new test),
`crates/skate-game/src/living_world/peds_tests.rs` (dump test extended).

**Verification.** New `the_mirrored_turn_mirrors_the_full_pose_and_never_flips_the_hips` (skate-core): a rig whose
hips reference carries the mode 1 literal, a turn clip with a zero hips delta and an asymmetric leg; the hips
stay at the reference at every tick including mid-blend, and out of the blend the left turn equals the right
turn's full pose mirrored. Fails before the change (hips flipped from tick 1), passes after. skate-core
`living_world::peds` 20 passed; skate-game `living_world` 26 passed; release build of `skate3rust` succeeds.

**Open.** Retail's ped motion graph tree order (where `AddBindPose` sits against the mirror node) was not read;
the fix follows the data (the reference is symmetric only as a full pose). Walk and blend poses rendered upright,
but a subtler twist in game (limb roll on the GLB followers, bones without clip data) is not excluded until the
user looks again.

## Peds rendered warped (bone frames twisted 90 degrees), 2026-10-05

**Problem.** After the left-turn fix the user still saw warped peds: "Peds look bad still. they are warped and not
quite right. we need to relook at the retail code and see what we are doing wrong. Honestly the recomp doesn't do
the best job of rendering them either, but ours is considerably worse at the moment." / "they look terrible".
Offline renders showed the same on every model and in every pose, the reference pose included: pinched waist,
wide bowed hips, twisted shoulders and arms.

**Root cause.** The skin was `bone global x render_basis x inverse(GLB bind)`. `render_basis` (90 degrees about the
bone's own x axis) belongs to the skater GLBs, whose converter (`character_glb.py`) bakes the matching
`basis_transpose` into their bind matrices so the two cancel. The ped GLBs (`living_world_models.py`) keep the
retail model's bind matrices unchanged, so nothing cancelled it: every ped bone turned 90 degrees about its own
axis. Spine bones have x up (the torso turned -90 degrees about the vertical), leg bones have x down (the legs
turned +90 degrees), so torso and thighs were 180 degrees apart at the hips and linear blend skinning collapsed
the waist. The clip data, the reference add and the mirror were not at fault.

**Evidence.**
- [data] All 51 ped GLBs against the rig's `PEDESTRIAN_RIG_TPOSE` globals: the bind frames match within 0.56
  degrees and 1.0 cm with no basis; with the skater basis every animated bone is 89.7 to 90.3 degrees off.
- Split offline (no game), `render_glb.py`: the bind pose renders correctly; the rig reference pose and idle /
  walk / stop frames through our pose path render warped with the skater basis and correct without it (same
  poses, same GLBs). Per-bone skin rotation of `male_adult_1` in idle before the fix: hips and spine about -90
  degrees about the vertical, thighs about +90 degrees.
- [code] `sub_827BA100` (the living-world presentation manager setup) names the rig `FullPedestrianACS`
  (`0x821AA1BC`) and `PedestrianBindPoseSQTs` (`0x821AA1D0`), allocates `cLivingWorldPresEntityManager::
  InvBindPoseMats` (`0x821AA1E8`, bones x 64 bytes via `sub_828D8170`) and fills each entry with the identity
  matrix (`0x82139A10` to `0x82139A40`) before uploading it. No per-bone basis or bone remap matrix appears on
  that path; the ped skin is driven by the animation rig's own bind frames, which is what the [data] check shows
  the model binds equal. Where retail forms the final palette (bone global x inverse bind) was not read.
- Reference look: the user's recomp sessions show peds only small at a distance (upright, normal proportions);
  the large figure in those shots is the player's skater.

**Change.** `skate-game::living_world::peds`: the joint globals handed to the shared binding are
`global x inverse(render_basis) x basis`, so the skin is `global x basis x inverse(GLB bind)`. `basis` comes from
`ped_bone_basis`, a pure function of the model: of identity (retail ped GLBs) and the skater convention, the one
whose `reference x basis` matches the GLB bind frames. Every shipped ped resolves to identity; a mod GLB written
the skater way still renders correctly, and a mod GLB written the plain glTF way (bind frames = rig frames) needs
nothing. Followers (fingers, face) use the bind in the rig's frames. No new tuning values (it is a correctness
fix); no setup re-run (the GLBs were right). Deterministic: the basis depends only on the model file.

**Files.** `crates/skate-game/src/living_world/peds.rs` (`reference_globals`, `ped_bone_basis`,
`ped_joint_globals`, `PedPuppet::basis`, binding and pose), `crates/skate-game/src/living_world/peds_tests.rs`
(new test; the dump writes the reference pose first), `crates/skate-data/tests/ped_anim_data.rs` (new data test),
local `.claude/skills/living-world/tools/render_glb.py` (`--bone-basis auto|none|skater`, auto = engine rule).

**Verification.**
- `ped_skin_rests_in_the_reference_pose_and_never_twists_bones` (skate-game): rig frames with spine x up and leg
  x down; for a retail-style and a skater-style GLB the basis is detected, the reference pose skins to identity on
  every bone, a 30 degree bend on the leg turns only the leg (30 degrees, torso 0); the pre-fix path gives 90.
- `ped_glb_bind_frames_are_the_rig_reference_frames` (skate-data, data-gated): all ped GLBs, every animated bone,
  bind frame vs reference frame under 2 degrees; the skater basis is at least 45 degrees off.
- Renders `.local/research/npc/fix10-out/before.png` / `after.png` (reference, mid walk, into stop, idle on
  `male_adult_1`, `female_adult_1`, `male_jock_2`).

**Open.** The final palette multiply in retail's ped renderer was not traced to an address. Tints are still not
drawn (separate item). Clip quality vs the recomp should be judged by the user in game.

## Peds walking in place at fixed spots, one standing on a wall top, 2026-10-05

**Problem.** User: "a bunch of them got stuck walking over some type of border and seem to still have pathing
issues." / "it just looks like a seam or perhaps an error in the navmesh at that spot? hard to tell, they just get
lined up and start walking in place in specific spots. Its shown in several videos" / "there is also a random woman
floating at the end of this one which also shows the people walking in place issue". Video: at the DownTown ramp
with the long blue rail (player near [-254, 41, 100]) a woman stands animating on the top edge of the tall wall for
8+ s.

**Root cause** (four ped navigation faults, one render feedback; tags as above):
1. **Tile seams.** The NavPower tiles stop short of their shared border, about 0.12 m each side [data]: at the
   user's spot polygon 7551 ends at z 99.9 and 6035 starts at z 100.14. Setup links the two sides, but a step was
   kept on the mesh by locating its end point alone; a point in the gap snaps to the closest edge, which for the
   first half of the gap is the edge the ped just left. A ped at a seam was put back each tick (counted as "on the
   mesh", so it never re-planned) and walked in place; peds heading the same way lined up behind it. 4449 seams in
   DownTown; the old rule gets across 188 of them.
2. **One-sided seam links.** Setup's stitch links the short polygons along one tile border to the long edge across
   it, but the long edge records at most one of them (6034's border edge links none; 7553 and 7554 link to 6034).
   Paths and steps from the long-edge side could not use those links.
3. **Corners left early.** Path corners are mesh boundary vertices (the mesh is already shrunk by the agent radius).
   A ped took the next corner as soon as it was 0.5 m from the current one; from the wrong side of the vertex the
   straight line to the next corner runs into the boundary edge head on, so the step slid along the edge by almost
   nothing. 415 walking-in-place episodes in a 30 ped x 120 s run round the ramp with 1 fixed, 64 with 1 to 3.
4. **Side-steps into walls.** A ped walking round another one side-stepped at 90 degrees even inside a 0.24 m wide
   strip (7626), into the boundary (31 episodes left before this was fixed).
5. **Floating.** The render ground probe (a line from 3 m above the ped down) wrote its hit back into the navigation
   position. DownTown has 661 separate navmesh components, many single wall-top or planter polygons over the ground
   (6092 at y 33.9 over 6104 at y 30.9); the probe could hit such a ledge, and the point query (any polygon under
   the point within 4 m, before any edge point) then put the ped on the unconnected wall-top layer, where it walked
   in place.

**Retail.** The ped's NavPower bot (`ped+5940`) moves over the bot's own polygon graph: the mover hands it the
destination (`sub_82C47378`) and reads its steering back (`sub_82C47198`) [code, pm3]; reachability is the graph's
connectivity bitset (`sub_82C464F0` -> `sub_82926BA8`) [code]. A bot does not leave its connected graph and the
runtime joins tiles across their borders. NavPower's path follow and steering internals are not decoded; the rules
below are ours, stated as such.

**Change.**
- `NavMesh::move_along` (new): a step moves over the polygons linked to the ped's own polygon (breadth first round
  the move): inside one of them, the step is taken at its height; in the strip across a linked edge (a seam gap,
  within the snap radius) the step is kept; otherwise it slides onto the closest edge. Never onto an unconnected
  layer. `NavMesh::point_on`, `NavMesh::clear_line` (straight walk over the surface).
- Links in both directions per edge (`edge_links`): a one-sided stitched link is added to the other polygon's
  closest edge; A* and the portals use them.
- `NavMesh::locate`: a polygon under the point more than the agent step height (0.2 [data], agent block) away in
  height competes with nearby edge points by vertical plus horizontal distance (a ground point in a seam gap stays
  on the ground, not the wall top above).
- `PedNav`: `poly` (the polygon the body stands on, stable per ped); a corner counts as passed within the corner
  radius only once the next corner is in straight reach over the surface; a side-step needs one agent diameter of
  room, else the ped yields (and re-plans after the patience, as before).
- `constrain_step` uses `move_along`; `constrain_move` takes the tracked polygon.
- Game (`living_world/peds.rs`): steps from the tracked polygon; a step that slid to under a quarter of the wanted
  distance counts as refused (re-plan after 3 s instead of walking in place); spawns go onto the navmesh at the
  record's height first (ground probe only without a navmesh); the ground probe is render only, in a window of one
  agent height (1.6 [data]) up and down round the navmesh height.

**Moddability / multiplayer.** No new tuning: the values come from the navmesh's agent block (step 0.2, height 1.6),
which a mod map's navmesh sets (`NavMeshInput.agent`); corner radius and patience stay in `WanderParams`. Pure
functions of the mesh and the ped's state, ties broken by polygon index; `PedNav::poly` is plain data.

**Files.** `crates/skate-core/src/living_world/peds/{nav,wander,nav_tests}.rs`,
`crates/skate-game/src/living_world/peds.rs`, `crates/skate-data/tests/ped_nav_data.rs`.

**Verification.** skate-core `living_world` 109 passed (new `steps_cross_tile_seams_and_never_jump_layers`, with the
old rule as a control). Data-gated on the user's DownTown export: `downtown_tile_seams_are_crossed_without_layer_jumps`
(4449 seams: crossed 4193, old rule 188; layer jumps 0, old rule 47; the rest are seams into slivers a few cm wide
that a straight walk leaves through their boundary), `downtown_peds_at_the_ramp_never_walk_in_place` (30 peds x
120 s round [-254, 41, 100]: 0 episodes of 4 s walking without getting 0.3 m, none leaves its connected surface);
the other ped_nav_data and living_world_data tests pass. skate-game `living_world` 43 passed. Not checked in game.

**Open.** Setup's stitch itself could record both sides (re-export); NavPower's path follow / steering and local
avoidance are not decoded; 1-polygon islands are still valid spawn points if a census record lies on one.

## Ped clothes drawn in their mask colours (looked like mixed outfits), 2026-10-05

**Problem.** User: "after further review some of the ped models are still not right." / "they aren't squished
anymore though" / "The three women at the start of this video are definitely not wearing the right clothes". The
women (log readout: `female_granny_1`, `female_teenager_3`, `female_skater_1`) wore a dark red / brown coat or top
with bright blue shorts, skirt or trousers; `male_teenager_1` and `female_business_3` (later in the same video) wore
the same flat red top and blue trousers. User on `female_business_3`: "Im pretty sure this npc isn't being rendered
corectly".

**Root cause.** Not mixed parts: every ped recipe holds one body (`Rostral`) and at most one `Hair` part, one mesh per
LOD [data, all 51 recipes], and each ped in the video matches its own GLB. The red and blue are the atlas's tint
masks, drawn raw:
1. The loader read model fields `tints_a` / `tints_b` that the export never had (the fields are
   `secondary_colours` / `chassis_colours`), so every ped got the white default pair.
2. Nothing drew the tints (M2 left the shader mask undecoded).

**What retail does.**
- `sub_827B4170` reads, with one rand `r`, `secondary_colours[r % n]` (`Hash_DF76D7D773857EDB`, stored at +80 of
  the ped's 112-byte presentation slot, `sub_827B3BF8`) and `chassis_colours[r % m]` (`Hash_12026E2EED18CC8D`, +96)
  [code].
- Ped body materials are type `pedestrian_high_stamp` (one LOD of one recipe `pedestrian_low`); hair and pro
  clothes are `marquee_hair` / `marquee_cloth` / `cac_alpha` [data, recipe XML]. The ped pixel shaders
  (`shaders_final.big`: `livingworld_stamp_defaultPS`, `defaultlivingworld_defaultPS`; read with an offline Xenos
  microcode decode of the `ucode.h` layout) both do [code, shader]: `lin = diffuse^2`; when `G^2 < 0.001225` (a
  literal) the texel becomes `R^2 x i_colorize_red + B^2 x i_colorize_blue`, else `lin`; lighting; output
  `sqrt`. Constant table: `i_colorize_red` c22 / c15, `i_colorize_blue` c23 / c16.
- Which tint feeds which constant [data]: the root `default` model record holds secondary `(1, 0, 0)` and chassis
  `(0, 0, 1)`; only secondary -> red, chassis -> blue makes that the identity. The CPU upload of the two constants
  was not traced (the parameter handles at `0x830B9C10` / `0x830B9BF4` are only referenced by their static
  initialisers, which register them by name through `sub_82531428`); the table values are taken as they are.

**Change.**
- `skate-core::living_world::peds::colorize`: the shader rule (`colorize_texel`, `colorize_rgba8`,
  `MASK_GREEN_SQ_MAX`, `colorized_shader`); `choice`: doc of the field mapping, `PedOverrides::model_tints` (a mod
  replaces a model record's palettes; same one-rand pick; `PedOverrides::default()` restores retail).
- `skate-data::ped_anim`: `tints_a` = `secondary_colours`, `tints_b` = `chassis_colours` (raw hash keys accepted).
- `skate-game::living_world::peds`: `present_ped_tints` (after `present_ped_looks`, before the pose): bakes the rule
  into a copy of the body diffuse per (material, tint pair), shared by peds with the same pair, freed with the last
  user (weak ids); `ped_material_colorized` picks materials by the export's material type, or the `Rostral_` body
  slot for GLBs exported before the type was written.
- Converter: `living_world.recipe_shaders` reads `<mat id type>` from each recipe's XML twin into `models.json`
  (`shaders`); `living_world_models.write_glb` writes it as material extras `{"shader": ...}`. A mod GLB opts a
  material in with the same extras.

Multiplayer: the pair follows from the spawn record's seed (unchanged draw order), the texture from the pair.
Moddability: palettes are table data (content overlay of `secondary_colours` / `chassis_colours` by model record),
`PedOverrides::model_tints` by model record id, material opt-in by extras; Lua surface with the `sdk.living_world`
ped calls (mod milestone).

**Evidence / verification.** Renders (`render_glb.py --ped-look MODEL:INDEX`, same rule) of the five models in the
video, untinted vs three palette entries each: coherent outfits (grey / white / tan tops, dark jeans, a brown or
green coat over a checked skirt). Tests: skate-core `living_world::peds` 34 passed (colorize rule, mod palette
override); skate-game `living_world` 45 passed (material selection); skate-data `ped_anim_data` 5 passed on the user's
tables (root pair is the identity, every ped look takes its model's palettes); asset_pipeline living-world unittest 32
OK. Not seen in game yet. Setup: old exports work through the `Rostral_` fallback; a `livingworld` refresh writes
the exact material types.

**Separate items, not this cause.** `female_business_3` has no `Hair` part (her ponytail is in the body mesh and
renders with it offline); her flat, washed-out head in the video is not a missing part (likely lighting on our standard material; not checked).
`male_teenager_1`'s grey shoulder patch is the sleeve trim, a non-mask region of his atlas (the same in the GLB);
his stiff arms-out walk is the animation side (open).

## Peds floating in the air all over the map (2026-10-06)

User: "There were MANY floating peds during my last play session. Not on geometry that is visible, literally floating int he air." / "IT IS NOT JUST ON PROPS GOD DAMNIT, IT IS ALL OVER THE PLACE".

- **Cause:** the ped render height (fix 17) took the first hit of a line from one NavPower agent height (1.6 m) above the navmesh down to 1.6 m below. Any collision within 1.6 m overhead (awnings, ledges, signs, invisible collision) won, so the ped was drawn standing on it. Video 2026-10-06 10-00-44 at 49 s and 51 s.
- **Change:** the upward search is the NavPower step height (agent block [2], 0.2 m [data]); NavPower keeps its polygons within one step of the walkable floor. Downward stays one agent height. Not retail yet: retail's own ped render placement is not decoded.
- **Evidence:** data test `ped_render_ground_does_not_lift_onto_overhead_geometry` over all 36,443 DownTown polygon centres: drawn more than 0.3 m above the navmesh at 195 polygons before, 0 after.
- **Files:** `crates/skate-game/src/living_world/peds.rs` (`ground(up, down)`), `peds_tests.rs`.
- **Logging (always on):** `PED_FLOATING` (warn, every 2 s per ped, once per ped per 10 s) when a ped is drawn more than 0.3 m above the floor under it or over no floor: ped, model, drawn position, navmesh height, floor height, gap, polygon and area code. The ped readout (count, nearest ped, player position) now runs every 2.5 s without debug mode. User: "im tired of you saying you can't see the floating pedestrains". A data check found 167 walkable DownTown polygons (areas 17 and 161) more than 1.6 m above the collision floor or over none; the log names them when a ped walks there.
- **Peds spawned at the player's height (2026-10-06):** the logs showed nearly every floating ped had no navmesh polygon (32 of 34, then 44 of 44). The census ring point carries the observer's height (`census::ring_point`); where the navmesh has no floor within `locate_height` (4 m) of it, the ped kept that height and hung in the air or under the ground. With a navmesh, such a spawn is now released like an unresolved look and the census tries another point next pass. Not retail yet: retail's spawn validation is not decoded. Session after the fix (about 6.5 minutes): 0 `PED_FLOATING`, the ped count stayed at its cap (14 to 15).
