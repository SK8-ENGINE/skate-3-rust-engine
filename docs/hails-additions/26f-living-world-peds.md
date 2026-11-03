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
local `tools/claude-skills/living-world/tools/render_glb.py` (`--bone-basis auto|none|skater`, auto = engine rule).

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
renders with it offline); her flat, washed-out head in the video is not a missing part; since the retail ped material (2026-10-25) her whole mesh uses it, and the check on 2026-10-29 found nothing wrong (`.local/research/ped-flat-head-stiff-walk.md`).
`male_teenager_1`'s grey shoulder patch is the sleeve trim, a non-mask region of his atlas (the same in the GLB);
his stiff arms-out walk is gone: a pose-dump render mid walk (2026-10-29) shows arms down and the same stride as `male_teenager_2`; his rig matches the other peds.

## Peds floating in the air all over the map (2026-10-06)

User: "There were MANY floating peds during my last play session. Not on geometry that is visible, literally floating int he air." / "IT IS NOT JUST ON PROPS GOD DAMNIT, IT IS ALL OVER THE PLACE".

- **Cause:** the ped render height (fix 17) took the first hit of a line from one NavPower agent height (1.6 m) above the navmesh down to 1.6 m below. Any collision within 1.6 m overhead (awnings, ledges, signs, invisible collision) won, so the ped was drawn standing on it. Video 2026-10-06 10-00-44 at 49 s and 51 s.
- **Change:** the upward search is the NavPower step height (agent block [2], 0.2 m [data]); NavPower keeps its polygons within one step of the walkable floor. Downward stays one agent height. Not retail yet: retail's own ped render placement is not decoded.
- **Evidence:** data test `ped_render_ground_does_not_lift_onto_overhead_geometry` over all 36,443 DownTown polygon centres: drawn more than 0.3 m above the navmesh at 195 polygons before, 0 after.
- **Files:** `crates/skate-game/src/living_world/peds.rs` (`ground(up, down)`), `peds_tests.rs`.
- **Logging (always on):** `PED_FLOATING` (warn, every 2 s per ped, once per ped per 10 s) when a ped is drawn more than 0.3 m above the floor under it or over no floor: ped, model, drawn position, navmesh height, floor height, gap, polygon and area code. The ped readout (count, nearest ped, player position) now runs every 2.5 s without debug mode. User: "im tired of you saying you can't see the floating pedestrains". A data check found 167 walkable DownTown polygons (areas 17 and 161) more than 1.6 m above the collision floor or over none; the log names them when a ped walks there.
- **Peds spawned at the player's height (2026-10-06):** the logs showed nearly every floating ped had no navmesh polygon (32 of 34, then 44 of 44). The census ring point carries the observer's height (`census::ring_point`); where the navmesh has no floor within `locate_height` (4 m) of it, the ped kept that height and hung in the air or under the ground. With a navmesh, such a spawn is now released like an unresolved look and the census tries another point next pass. Not retail yet: retail's spawn validation is not decoded. Session after the fix (about 6.5 minutes): 0 `PED_FLOATING`, the ped count stayed at its cap (14 to 15).

## An ended approach drops its route (2026-10-17)

Problem (laptop session 2026-10-17; user: "There were lots of peds stuck at random points where they were walking into
a wall or just wandering around in a small circle."): peds stood against something 3 to 12 m short of their target
with speed about 0; #111 kept one target for 20 s. Research b177 (`.local/research/b177-ped-obstacle-giveup.md`).

Root cause [code, ours]: ApproachWantTarget gives the nav a one-point route (`peds.rs`, the approach arm) and sets no
chase goal, and the "no brain move" arm only dropped a route when a chase goal had been set. So when the approach
ended, its point stayed, and every give-up (the stuck watch, the yield patience) re-planned that same point: `plan()`
with a route ignores `reverse` and clamps to the route's last point (`wander.rs`).

Retail [code, b177]: the mover re-targets only when its path is done (`sub_82E2CB08`, `sub_82E96728`), and a move's
path goes with the move. Retail has no time / distance give-up in the wander goal: event 4 ("reverse") is only posted
by the graph op CheckForTargetTimeout, which is commented out in `pedestrian_wander.xml`. Our stuck watch, corner stall
and patience stay as NOT RETAIL fallbacks.

Change: `PedMind::approach_route` marks the approach's route; `ended_move_drops_route` clears it (and a chase goal's
route, as before) on the first tick without a brain move. A mod's `PedRoute` is not marked and is not dropped.

Test: `an_ended_approach_or_chase_drops_its_route_once` (peds_tests). Not play-tested yet.
Answered 2026-10-19 (next section, b190): NavPower's path follower has no progress give-up.

## Peds pinned against the prop they want to use (2026-10-19)

Problem: "peds stuck walking into walls" (USER-ASKS 68, PR #52 next item 1). The newest laptop session
(`second-pc/HAILS-ROGLAPTOP/game-20261019-001947`, after the 2026-10-17 route fix) still has walking peds standing
still, now nearly all inside a prop plugin on their way to its waypoint: ped #563 55 s and #573 25 s in
`waypoint_newspaperbox`, #578 in `waypoint_sit`, each `blocked` 1 to 3 s. `PED_BLOCKED` names the prop itself: #573 at
(-261.19, 539.74), 1.045 m from its waypoint, pressing into `prop_DT_NewspaperBoxReclaimedBlack_1004` at (-261.80,
539.57) with `progressed=false`, until the plugin's 60 s `PluginTimeout` (graph data) ends the plugin. The waypoint
(`native-props/DownTown.json`, hotpoint `EB4A16533EACEC04`) is at (-262.22, 539.56) facing the box: 0.42 m in front of
the box centre, inside the box grown by the agent radius. The ped came from behind the box.

Retail [code, TU3; b177, b190 and a 2026-10-19 pass]:
- No give-up on missing progress anywhere in the ped's NavPower stack. Bot update `82E974A8`: re-requests the path
  when it is no longer valid (`82E95FD0`: path vfunc 4, portal flags, follower vfunc 0x70 = sub-path invalid,
  `82EA0398`), ends it on the last segment when the follower is done (`82E96F08`, mode 0 = done, so the wander goal
  picks a new target). Path follower (vtable `0x8232E428`, update slot 0x34 = `82EA0808`): states 1 turn in place,
  2 move, 3 arrive, 0 done, 4 link; the only timer is a 3.0 s start-up window (`+0x108`, `0x82063B08`). The bot's
  no-progress timer (`82E965D0`, +0x21C) has no reader (b190).
- Movement stays on the mesh: `82EA5C40` moves the agent in up to 10 sub-steps (`82EA4C20`, each returning the time
  left after an edge), crossing into the next polygon or sliding along a boundary edge [code, medium on the slide].
- A resting prop is cut into the mesh (`82C477B0` -> `8293EF28`, record `{-1, 15.0 (0x820BD16C), obj+0x4C}`; the
  meaning of -1 / 15.0 is not decoded), and a bot destination is snapped onto the cut mesh within a 0.5 m window
  (`82C46348` / `82C460E0`, radius `0x8209975C`; b302). So a retail ped is never inside a cut, its path goes round the
  prop, and its path ends at the free point next to a waypoint that lies in the cut; TargetWaypoint's slide
  (`slideDistance` 1.0 m in `moveontowaypoint.xml`) covers the rest. Only `sit` uses
  DisableCollisionsWithBehaviourSource, so a newspaper box user never ignores its box.

Root cause [code, ours, three parts]:
1. `NavObstacles::detour` picked the leg's own start corner as the next corner (its zero-length leg never hits), so any
   detour that needs a second corner repeated the first up to `max_detours` and the path went straight from it.
2. A route point inside a cut stayed the path end, so the last leg always entered the box.
3. `detour` left the leg unbent when the start was inside the box grown by radius plus `detour_margin`, which is where
   a ped stands after the step check stopped it at the box: every re-plan (stuck watch, patience) aimed through the box
   again.

Change (`crates/skate-core/src/living_world/peds/obstacles.rs`, `wander.rs`): the start corner is never a candidate;
a route point inside a cut snaps to the nearest free point within `ObstacleParams::target_snap` (0.5 m, retail window;
`snap_out`), else stays (no path, short fan next, as before); "start inside" means inside the body's rectangle (radius),
and from inside the margin the corner legs are tested against that rectangle; a leg ending inside the margin only (a
snapped end) is tested against the body's rectangle. Mod key `world_tuning` `ped_obstacles.target_snap` (dump and
merge in `skate-mods` / `skate-game` `world_tuning.rs`).

Tests (`obstacle_tests.rs`): `a_ped_reaches_a_waypoint_in_front_of_its_prop_from_behind` (the #573 geometry, from
behind and from pressed against the back; before the change the ped ended 1.03 m from the waypoint, the log's 1.045 m)
and `a_target_inside_a_cut_snaps_out_only_within_the_snap_radius`. `cargo test -p skate-core --lib living_world::peds`
148 passed; `skate-mods` 106 passed with only the known `skyline_physics` failure (gitignored GLB); `skate-game`
checks. Not play-tested yet.

Changed 2026-10-19 (next section): the body step applies a slide however short, as retail's mesh walker does. Still ours: the stuck watch, corner stall and patience stay NOT
RETAIL fallbacks. Open: the cut record's -1 / 15.0 (hard block or a cost), the sit case #578 (same mechanism assumed,
not checked against its seat geometry).
## Peds slide along walls instead of standing pinned (2026-10-19)
- **Problem:** peds walking into a wall or street furniture at a shallow angle stood pinned (laptop sessions; asks 84,
  item 1 of the #52 list). User: "no I don't really see peds stuck against a wall in retail."
- **Retail** ([trace] own scripted recomp run, `.local/research/b325-wall-stuck-recomp-results.md`): at two of our
  worst spots, 15 peds over ~8.5 min of game time, the still-timer obstacle chain (`82E97118` -> `82EA17E8`, b315)
  never fired once: retail peds do not get pinned, so no unstick rule is needed. ([code] Ghidra + recomp, b311 /
  `.local/research/b331-ped-slide-port-plan.md`) The mover `82EA5C40` runs up to 10 sub-steps of `82EA4C20`: a move
  that hits a boundary edge is placed on the hit point and its velocity projected onto the edge, however small the
  rest; head-on the projection is 0 and the ped stands on the edge. No progress ratio.
- **Change:** `peds.rs` body step: the `progressed` test (a slide under 25 % of the wanted step counted as refused)
  is gone; `PED_BLOCKED` no longer prints it and now logs only real refusals (off the mesh, a prop corner with no free
  slide, another ped, a red light). On a straight edge our `move_along` snap gives the same point as retail's
  projection [inference: geometry, not traced].
- **Test:** `a_ped_walking_into_a_wall_at_an_angle_slides_along_it` (80 degrees into a wall: every tick's short slide
  is applied and the ped moves along the wall; head-on it stays on the edge).
- **Open:** inside / outside wall corners (retail's on-edge branch to the next edge not read); the stuck watch still
  uses our values (retail's still timer 0.13 m / 0.5 s re-paths to the same destination, b314); needs a play test.
## Peds keep their target and re-path to it, as retail (2026-10-19)
- **Problem:** our stuck watch (0.3 m in 2 s), the yield patience (3 s) and the host's refused-step patience gave a
  ped's target up and planned away from it. Retail never gives up a walking ped's target for lack of progress.
- **Retail** ([code] research `.local/research/b332-ped-stuck-watch-parity.md`, b336; b190, b313-b315; [trace] b325):
  retail never gives a walking ped's target up. The bot's still timer (`82E97118`, 0.13 m / 0.5 s, re-requests the
  same destination `82E957F8`) runs in bot mode 4 only, not for walking peds (b336);
  an invalid path is re-requested every tick and only a failed request ends it (then the bot picks a new target).
  The walking path step `82EA0808` has no neighbour handling at all (b336: every callee read), so our yield, sidestep
  and push are ours; what keeps retail walking peds apart (acceleration writer, character-layer collision) is not read; the 2.0 s "detour home" of
  `82EA4408` belongs to the bot's mode-0 controller (no path: a standing ped pushed off its spot), not walking peds
  (`.local/research/b335-ped-crowd-detour.md`).
- **Change:** `PedNav::repath` (same target, a route's index kept; no path: drop the corners and skip the long fan, so
  a new target is picked). The stuck watch re-paths after 0.5 s within 0.13 m, once per spell (OURS on walking peds,
  with the still timer's values). The yield (`yield_patience` 2.0 s, NOT RETAIL: the wait is ours, b335) and the host's refused steps re-path instead of dropping the
  path. NOT RETAIL, kept as a safety net: `give_up_seconds` (2.0 s, 0 = off) still gives the target up when the ped
  stays pinned; to be switched off once a play session shows no give-ups. Mod keys: `ped_nav.give_up_seconds`,
  `ped_nav.yield_patience` (new), `stuck_distance` / `stuck_seconds` now the still timer.
- **Tests:** `a_still_walking_ped_repaths_to_its_target_once_then_gives_up` (retail values, one re-path per still
  spell, same target, a new spell after moving, no give-up when off); `a_pinned_walking_ped_gives_up_and_plans_away_from_its_target` (the safety net at 2 s); `ped_nav_is_mod_reachable_and_reset_on_disable` (give-up off).
- **Open:** what keeps retail walking peds apart (b336 open: the +0x20 acceleration writer, slot +0x3C, character-layer
  collision; a recomp trace of two walking peds meeting would settle it); a standing ped pushed off its spot
  (mode 0: path home after 0.455 m and 2.0 s, b335) is not ported; whether walking ambient peds run the still timer
  at all (b325 never saw it fire).

## Peds walking in place next to a dropped prop (2026-10-19)

**Problem.** Laptop session 2026-10-19 18:04, user: "Every now and again I see one just walking in place for a moment
before stopping and changing course", and later "they were not next to the prop always". The log: `PED_BLOCKED` with
small dropped hand props (a pop can, a purse, a water bottle, a phone, a coffee cup, 0.3 to 0.8 m away; 4 of 5 were the
ped's own just-released prop) and one ped stuck about 29 s against a garbage can (0.64 m from its centre), each step
going about 0.03 m toward it.

**Retail (research b349) [code].** A ped's hand prop is an ordinary DMO. While it is in the hand its obstacle-off word
(component +0x109C, set by the attach slot `82C56B00`, cleared by the release `82C56C70`) keeps it out of the cuts
(`82C48648`); from the release frame on it is a normal DMO cut (half extents at least 0.2 m, `82C477B0`) for every ped,
the dropper too. Retail peds re-plan on the cut mesh, snap their destination out of a cut within 0.5 m and slide along
cut edges; they are never inside a cut. Ours already matches the hand-prop rule (a held prop is not a prop body, a
released one is listed by `obstacle_boxes`).

**Root cause (ours).** A ped standing inside a footprint grown by its radius (a prop came to rest next to it) gets every
step refused: `step_ok_ignoring` only allows steps away from the footprint centre, `resolve_step_ignoring` finds no
slide from inside, `detour` leaves the leg, so the path keeps aiming through the prop and the walk clip plays in place.

**Change (ours, for retail's outcome).** The walker (`peds.rs`) checks whether the ped stands inside a footprint (not
while its plugin ignores its own prop); if so it steps toward the nearest free point (`NavObstacles::snap_out`, within
0.5 m) at its walking pace and re-plans from there. `PED_BLOCKED` gains `inside=`. Test
`a_ped_inside_a_fresh_cut_can_walk_out_toward_the_nearest_free_point`. Not play-tested yet.

**Open.** What NavPower does with an agent a new cut covers (a recomp hook on the release `82E3EBE0` and the first
cut `82C477B0`, following the dropper for 2 s, would show it).

## Ped material: the retail shadow side (2026-10-25, USER-ASKS 352)

**Problem.** The user (2026-10-16): "the lighting of peds in the recomp are much better and smoother ours in the rust
engine are rough and blocky feeling dark patches", "mainly on the shadowed side". Cause 1 (DXT1 normal maps read as
two-channel, doc 26g "Ped normal maps") was fixed and play-tested ("the shadows look much better"); this is cause 2.

**Retail [code, shader microcode; data, vault; research b614, `.local/research/b614-ped-character-sh-fill.md`].** The
ped body program `livingworld_stamp_defaultPS` (material types `pedestrian_high_stamp` / `pedestrian_low`, TU3
`shaders_final.big`) has no spherical harmonics fill, no ambient cube and no environment map: its constant table has
`m_params` rows 0..6, the light direction, the eye, the shadow matrices and the colorize pair, and no `i_shmult` (the
skater programs `cac_cloth_defaultPS` / `defaultcharacter_defaultPS` have it). The light: `albedo^2 x (sat(N.L) x
m_params[1].rgb x shadow + m_params[5].w x (0.1, 0.175, 0.3) + rim x m_params[6].rgb) + spec`, rim =
`(1 - N.V)^m_params[0].z`, then `x m_params[0].y` and the exposure tail, the car shader's layout (doc 26h "Car
materials"). Ped body parts bind `character.livingworld_pedestrians_stamp` (rows `0 (0.05, 1, 0, 3)`, `1 (1.5, 1.4,
1.2, 1.6)`, `2 (0, 0, 0, 9)`, `3 (0.005, ...)`, `5 (0, 0, 0, 0.275)`, `6 (0, 0, 0, 0.3)`), so the whole shadow side
is `albedo^2 x (0.0275, 0.048, 0.0825)`: flat, dark, slightly blue and independent of the normal, which is why the
recomp looks smooth. Ours drew peds with Bevy's StandardMaterial under a white ambient whose diffuse and specular
parts vary with view angle and roughness, so normal-map detail showed on the shadow side.

**Change.**
- Setup (`living_world_models.py`): each ped material's extras carry `attrib_material` (the part arena's
  `AttribulatorMaterialName`, read as the car exporter does) and `params` (its vault `m_params` rows, resolved with
  `living_world_vehicles.material_params`, one vault table per export). Refresh Game Files once.
- Setup fix found on the way (`install.py`): the ped hand props live in `private/living_world` (the livingworld
  group's folder) but were only written by the maps step, so a livingworld-only refresh published without them (the
  game then shows "Game files need a refresh: ped hand props"). A livingworld-only refresh now prepares the DMO
  catalog (worlddmo.big only) and writes them itself.
- Engine: `living_world/ped_material.rs` + `ped_material.wgsl` (`PedMaterial`, the program above; the colorize tint
  stays baked into the diffuse copy per pair, the same rule). `present_ped_tints` gives opaque parts of the stamp
  program the ped material when the retail lighting runs and the part has rows; masked parts (hair, accessories) keep
  their glTF material until their programs are ported. `update_ped_light` follows the character lighting's light.
  The NPC fade (`npc_skaters::present_fade`) is generic over the material (`FadeMaterial`): the ped material fades
  through its output alpha, retail's `i_params.x`.
- Data / mods: the rows are data per stable vault name; `PedLooks::material_params` (name -> rows) wins over the
  export's, and a mod ped GLB can name a row (`attrib_material`) or carry its own `params`. The ambient literal is the
  car port's constant (`vehicle_material::AMBIENT`).

**Verification.** `ped_material::tests::stamp_rows_give_the_retail_shadow_side_and_mods_override_them` (ambient =
0.275 x literal, rows from the extras, a mod's rows win, masked parts excluded); setup
`test_living_world.Models.test_skinned_two_lod_glb_from_a_recipe` (name and rows in the extras); the living world and
replay suites (fade).

**Open.** The hair program `defaulthair_defaultPS` and the second specular lobe (colour 0.005 for peds) are not
ported; the CSM / world-shadow combine of the program (`g_CSMSelfBias.w`) is partly decoded; the ped ground shadow
layer (cause 3 of the todo) is separate.

## Ped speed rows: brisk walks and slow / fast chase runs (2026-10-29)

Problem: every moving ped walked the one walk cycle or ran the one chase run cycle. Retail has a brisk walk, a slow
and a fast chase run, and some ped types (female, bum, zombie) run differently.

Root cause (research `.local/research/b682-ped-motion-intents.md`, checked at the source): one poster, `sub_82E29D98`,
picks the locomotion state and its cycle tier from a speed. Each `livingworld_entity_animation` set has a speed row
table (field `Hash_9C9A4F2CB1CC45DE`, 8 words a row: float threshold, then string-pool offsets of the state, the tier
intent and 5 turn-compatible previous states, offset 8 = empty) [data, resolved from `skatercollections.bin`]. A speed
above `[0x8231A844]` (1.0) gets `[0x820C6D98]` (0.25) added (`fcmpu` / `ble` / `fadds` in the recomp asm), then the
highest row at or below it wins [code]. Default set: 0 Idle, 1 Shuffle, 2 Walk, 4 Walk + Brisk, 6 ChaseRun + Slow,
8 ChaseRun, 10 ChaseRun + Fast; female: up to 4 the same, then 8 ChaseRun + Slow only; bum / zombie: 8 ChaseRun with no
Slow / Fast; marquee sets: Idle only [data]. The motion graph enters `WalkCyc.Brisk` (`FwdBriskWalkCyc`) and
`ChaseRunCyc.Slow` / `.Fast` (`FwdChaseRunSlowCyc` / `FwdChaseRunFastCyc`) at the current cycle's branch window with a
0.1 s blend, back to the default cycles the same way (`motiongraph_walk.xml`, `motiongraph_chaserun.xml`) [data].

Change:
- Setup (`tools/asset_pipeline/living_world_anim.py`): `speed_rows` next to the raw field, each row
  `{threshold, state, tier, turn_from}`. Old installs need Refresh Game Files (without rows the ped walks, or runs when
  fleeing, as before).
- `skate-data/src/ped_anim.rs`: `PedAnimSet::speed_rows`, sorted.
- `skate-core/src/living_world/peds/anim.rs`: `SpeedRow`, `Tier`, `SpeedBump` (the 1.0 / 0.25), `speed_row`,
  `moving_intent`; the player's `tier` picks the walk / run cycle, a change swaps the cycle at the branch window.
- `skate-game/src/living_world/peds.rs`: a moving ped's intent and tier come from the brain's speed
  (`speed_suggestion`) through the rows. `PedBrainSettings::speed_bump` holds the bump. A mod reaches the rows
  through the set's table data.
- OURS, not retail yet: a fleeing / chasing ped without a brain speed runs the set's untiered run row, or its first
  run row (so the female set runs Slow), because the flee speed is not decoded. Idle / Shuffle rows keep the nav's walk
  (no Shuffle state yet).

Verification: tests `the_speed_rows_pick_state_and_tier_after_the_bump`,
`a_tier_change_swaps_the_walk_cycle_at_the_branch_window` (skate-core), `test_speed_rows_gain_named_states_tiers_and_turn_lists`
(setup), the data-gated `every_census_entity_resolves_a_look_and_animation` (7 default rows, female ends in a Slow run,
the three tier clips resolve in the stock set).

The run's own clips (research `.local/research/b684-ped-run-tired-graph.md`, same day): Idle or Walk into a run plays
`Stand2ChaseRun` (0.25, out at 0.1 into the tier cycle; `Locomotion::RunStart`), a turn while running plays
`ChaseRunTurnR45..R180` (0.15, left mirrored, at the run cycle's branch window, out at 0.1;
`RunTurnLeft` / `RunTurnRight`; the wander now posts run turns instead of stopping to pivot), a stop plays
`ChaseRun2Stand` (0.15) and a run back to a walk plays `Stand2Walk` (0.25) [data, `motiongraph_chaserun.xml`,
`motiongraph_idle.xml`, `motiongraph_walk.xml`]. A set without a clip falls back to the old path. Test
`a_run_starts_turns_and_stops_through_its_own_clips`; the clips resolve in the stock default set (data test).

Tired (b684, same day): RestFromChase Begin (`8269F4D8`) posts one packet with two stages, [Tired] then
[Tired + StopTired]; its End (`8269F608`) advances the stage (`+0x1C4 += 1`, read in the recomp asm) and removes the
packet, so StopTired arrives when the AI is rested [code]. Graph (`motiongraph_tired.xml`) [data]: from the run cycle
`ChaseTiredInto` at a branch window, from anything else straight to `ChaseTiredCyc` (blend 0.1); the cycle leaves on
StopTired at a loop end into `ChaseTiredOut` (out at 0.01); then Idle and Walk start their cycles directly (their
into lists have no Tired) and a run plays the plain `Stand2ChaseRun`. Ours: `PedAnimPlayer::resting` follows the
brain's `chase_resting` (Begin sets, End clears), states `TiredInto` / `TiredCycle` / `TiredOut`; OURS: the graph's
`TiredBranchInfo` is not decoded, the run cycle's own branch windows stand in. Test
`a_rest_after_a_run_plays_the_tired_clips_then_walks_on`; the three clips are the shared `NPC_TIRED_*` of all sets.

Shuffle (research `.local/research/b686-ped-shuffle-strafe.md`, same day): a speed in the Shuffle row (from 1.0; in the
stock data only TargetWaypoint's 1.0 near a plugin point, `moveontowaypoint.xml`, `brain.rs` TargetWaypoint) shuffles:
`Stand2Shuffle` (0.25, out at 0.1) from Idle / Walk / ChaseRun, `FwdShuffleCyc` (0.1), `ShuffleTurnR45..R180` (0.15,
left mirrored, out at 0.1), out through `Shuffle2Stand` (0.15) to Idle, `Stand2Walk` to a walk, `Stand2ChaseRun` to a
run [data]. Ours: `Intent::Shuffle` from `moving_intent`, states `ShuffleStart` / `Shuffle` / `ShuffleTurnLeft` /
`ShuffleTurnRight`, the wander posts turns from Shuffle, a shuffling ped can dodge. Test
`a_shuffle_speed_shuffles_turns_and_stops_through_its_clips`.

Strafe (b686, same day): BlockChasee sets the mover's strafe flag and target (the chasee) every update
(`sub_826A40E8` -> `sub_82E2C000`) and clears it at its End (`sub_826A4338` -> `sub_82E21B18`) [code]. While it is
set the poster posts Strafe: first a turn toward the target through the turn bucket with mask 0xCC (from 25 degrees
off a 45, from 70 a 90, nothing larger), else with a speed above 0 a step by the angle of the move direction on the
25 / 70 / 115 / 160 edges (Forwards, Right / Left 45 / 90 / 135, Backwards; `sub_82E2AFC0`, edges dumped) [code].
Graph: `Stand2Strafe` (0.25, out at 0.03) from Idle / Walk / Shuffle, `Chase2Strafe` from a run, `StrafeIdle` (0.1),
the step cycles `StrafeForwards` / `StrafeBackwards` / `StrafeR45/R90/R135` (0.15, left mirrored, at the branch
window), `StrafeTurnR45/R90`; out with no stand clip (IdleInto / WalkInto / ShuffleInto do not list Strafe), to a run
through `Strafe2ChaseRun` [data]. Ours: `PedBrain::strafe_target`, `anim::strafe_input` (edges in
`PedBrainSettings::strafe`), `PedAnimPlayer::strafe` and the Strafe states; the caller steps toward the block point
and turns by the clips (no face pivot); a set without `Stand2Strafe` keeps the old walk and pivot. Tests
`the_strafe_turns_to_the_target_then_steps_by_angle`, `a_strafing_ped_steps_turns_and_leaves_without_a_stand_clip`.
Read 2026-10-29 (research `.local/research/b695-strafe-leftovers.md`): `sub_82E2AB18` is the target minus the ped in 3D
(not flattened), so a chasee above or below changes the turn size; ours now takes the size from the 3D dot and the
side from the flat cross. `sub_82E2A6B0` steps toward the first path node more than 0.1 m away (3D), not the goal;
ours now uses the first nav corner past `node_reach` (`StrafeRules`, 0.1), else the block point. BlockChasee Begin
(`sub_826A4038`) sets neither the flag nor the target, as ours. Open: the stored chasee height, goal+0x30 (the
fallback step vector).

Open (b682 / b684 / b686): `Chase2StrafeR*` and start-with-turn into a run (`Stand2ChaseRunR*`, needs a turn and a
run intent in one stage), the transition freeze (`+0x84` / `+0x86`), the turn gate from the previous row's state, the
request masks; whether the mover's speed sample is the desired or the measured speed.

## Code reference: living_world/peds.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/peds.rs`; each comment there links its section here.

### Ped body tier

Pedestrians, body tier (doc 26, peds milestone M2): turns the population's pedestrian spawn / despawn records into visible, animated peds with footstep audio.

- **Look** ([`Pedestrian`]): the entity inside the census category and its model from the spawn record's seed (`skate_core::living_world::peds::choice`, retail `sub_826B8B88` / `sub_826BB058`), the model's tint pair (`sub_827B4170`), painted onto the body's mask texels by [`present_ped_tints`] (retail's ped shader rule, `peds::colorize`). GLB `private/living_world/models/ <recipe>.glb` (both LODs, parts, textures), or a mod's GLB from [`PedLooks`].
- **Animation** ([`PedBody`]): the ped animation player on the clips of `PedestrianSkeletonPres.abin`, stepped once per population world tick (1/60 s, `clock::RETAIL_TICK_HZ`) so a ped's state is a function of its spawn record and the population tick. Root motion moves the ped; it is snapped to the ground below (a line query, like the audio's ground material probe). The intent comes from navigation (M3, below); on a map without a navmesh a ped follows [`TestPath`] (idle, walk a few metres, stop, turn round).
- **Navigation** (milestone M3): the district's NavPower navmesh (`private/living_world/ navmesh.bin`, [`PedData::nav`]) and retail's `NoRoadWander` goal (`skate_core::living_world::peds::wander`: probe fans 40 m / 10 m, A* + funnel corners, re-target on arrival), ped-to-ped avoidance and separation, every step kept on walkable polygons. Retail ambient peds never use crosswalks (the road branch of `Pedestrian.xml` is unreachable in TU3); [`PedNavSettings::crosswalk`] = `WalkSignal` is a mod option that waits for the walk light of the shared traffic signal clock.
- **Dynamic obstacles** (fix 11, retail DynamicObject NavPower obstacles, see `skate_core::living_world::peds::obstacles`): [`PedObstacles`] holds every prop (the physics' prop bodies at their current pose, so a prop the player moved counts where it lies) and every mod body. At rest they are cut out of the walkable area (re-cut after moving more than a quarter of the smallest half extent, no cut while moving faster than 0.4 m/s or carried); targets inside a cut do not fit, paths bend round cuts, and a body is never stepped into ([`NavObstacles::resolve_step`]). Rules: [`LivingWorldSettings::ped_obstacles`] (`sdk.world.set_tuning("living_world", {ped_obstacles = {...}})`).
- **Skinning**: the GLB's 39 joints bind to the 50-bone rig by name (all match, data test); bones the clips do not carry (fingers, face) follow their rig parent with the GLB's bind offset. The skin matrix is `bone global x inverse(GLB bind)` with NO extra bone-local basis: the ped GLBs keep the retail model's bind frames, which are the rig's reference frames (data test `ped_glb_bind_frames_are_the_rig_reference_frames`), so the reference pose skins to the bind mesh. The skater's `render_basis` (its GLBs bake a matching basis into the bind) twisted every ped bone 90 degrees about its own axis (torso -90, legs +90 at the hips): the pinched waist / warped peds of fix10. [`ped_bone_basis`] picks the basis per model from its bind frames, so a mod GLB written the skater way still works.
- **Draw fade** (retail `sub_827C1188`, `skate_core::living_world::peds::fade`): opaque up to the model's first distance pair (45 m [data]) from the camera, gone at 55 m, plus a 1 s spawn fade in; the opacity goes into [`NpcFade::alpha`](super::npc_skaters::NpcFade) and is drawn by the shared NPC fade; at 0 the ped's scene is hidden. This is what keeps the census cull (70 m, a pure distance test) out of sight, as in retail.
- **LOD** (placeholder, the LOD pick is not decoded): `LOD0` within 45 m, `LOD1` beyond 55 m, hysteresis between; with the fade above `LOD1` only shows while fading.
- **Audio**: [`PedAudio`](crate::world_audio::PedAudio) with the model's voice, the clip's `LEFTTOEDOWN` / `RIGHTTOEDOWN` windows as `feet_down` and `BODYFALLTYPE` as `body_fall`, so #32's ped footsteps and body falls play.
- **Events** for engine systems and the planned `sdk.living_world`: [`PedEvent`].

Moddability: [`PedLooks`] (category entity lists, entity -> model / animation set, recipe -> GLB path) is the one place overrides go; restoring `PedLooks::default()` undoes a mod (new spawns use retail looks again). Multiplayer: nothing here decides a spawn; a client rebuilds the same ped from the same `SpawnRecord` and tick.

### Ped spawn without floor

The census ring point carries the observer's height, not the floor's (census::ring_point). Where the navmesh has no floor within locate_height of it (the player on a ledge, ramp or roof), the ped kept that height and hung in the air or under the ground (sessions: 32 of 34 and 44 of 44 PED_FLOATING peds had no navmesh polygon). The spawn is released like an unresolved look; the census tries another point next pass. Not retail yet: retail's spawn validation is not decoded.

### Skater vs ped contact

The local skater meets a near ped's body in its own solve (`ped_body::push_ped_proxies`, group 11): the reaction comes from those contacts, as retail's comes from the Havok contact (b185). A distance test that knocked the ped down about 0.35 m before the bodies met let a fast hit never bail the skater. Other skaters and peds without a body keep the distance test (NOT RETAIL).

### Takedown contact handling

A takedown under way (AttemptTakeDownTargetable / WaitForTakedownDecision, both not interruptable and without a Colliding transition, `pedestrian_attempttakedowntargetable.xml`): retail's contact callback `82E38FB8` only stores the reaction, the touch of the target decides the takedown (`826A4BE8`) and success drops the stored reaction (`826A4E50`), so the attacker never falls from its own lunge (b317). A contact by anyone else during the attempt is stored too and plays when the graph next reaches Colliding, after a failed takedown (`826A5068` leaves it stored; b326).

### Edge slide step

A step that slides along an edge is applied however short the slide is: retail's mover (`82EA5C40`, up to 10 sub-steps of `82EA4C20`) projects the velocity onto a boundary edge it hits and moves along it; head-on the projection is zero and the ped stands on the edge. It has no progress ratio. A refused slide under 25 % of the wanted step pinned peds walking into a wall at an angle (b311, b331; a recomp run never saw a retail ped pinned, b325).

### Ped render ground

The render ground is a line query round the navmesh height. It never feeds back into the navigation position (fix 17). Retail's probe reaches 1.6 m both ways, and b358 found no overhead hit within 1.6 m on any map. Retail's own ped render placement (who writes the ped matrix y, `82E221B0`) is not decoded (b360).

### Ped ground probe 82EA5F48

Retail's NavPower mover probe (`82EA5F48`) [code, b359]: cast 1 from point + L to point - L, with L the probe settings' length, 1.6 m on every ped call measured in the recomp [trace]; after a miss (`0x82257308` 4.0) cast 2 runs UP from the point to point + 4L and its end (a ceiling, if hit) is the top of cast 3, which runs down to point - 4L. Coarse navmesh triangles sit up to 1.7 m above the pavement (DownTown poly 7376) and up to 4.3 m under the road (b358); retail's peds always stand on the floor. b358: an upward reach of 1.6 m takes no overhead geometry on any map.

### Ped material tint rule

Whether a ped GLB material gets the tint: its retail material type from the export's material extras (`{"shader": "pedestrian_high_stamp"}`, `colorize::colorized_shader`); for a GLB exported before the type was written (no extras), the body slot `Rostral_*` (every shipped ped body is `pedestrian_high_stamp` / `pedestrian_low` [data]; only one ped hair uses the ped shader and is missed by this fallback until the next export).

### Ped bone basis

The bone-local basis between a ped GLB's joint frames and the rig's: the candidate (identity, the retail ped GLBs; or the skater GLB convention `render_basis`) whose `reference x basis` is closest in rotation to the GLB bind over the matched bones. Identity for every shipped ped model (data test); a pure function of the model, so deterministic.

### Ped collision broadcast

Collisions from the skater contact of the last steps (`PedEvent::Hit`): retail posts `collision` to every receiver (broadcast radius 0 = unlimited); a ped that was not the one hit fails its "ped itself" prerequisite and records `nearbycollision` instead (`sub_82E41060`). The instigator is the ped's last toucher (the contact record `+3184`), kept until its Colliding state begins.

### Moving seat

A seat whose prop moves (grabbed, pushed, knocked): retail keeps the ped's plugin and seat (research b102). The owned object reporting a move to its seated ped (`82E3DBE0`: |v|^2 > 2.5e-5, `0x822F8BE0`) makes it jump off forward; the graph then walks it to the seat's new place and sits it again (the locked waypoint is re-read every evaluation, `826AB9C0`). The seat's own speed stands in for the object's vector: its move over one step of `SEAT_PUSH_STEP_SECONDS`.

### DMO plugin check

The DMO's plugin check (`82C56DB0`, b136 / b137): moved more than its type's max distance, or turned past its per-axis tolerance -> off (seated peds evicted); else each seat must stand on pavement navmesh. NOT RETAIL YET: when retail runs it (DMO vtable +264's caller) is open, likely on a move / settle message; ours every think, only for a prop away from its authored pose.

### Seated owner jump off

`82E3DBE0`: the seated owner (OwnPluginObject, `brain+3277` bit 0x01) of a moving object jumps off forward: intent Avoid / Jump / Forward -> motiongraph_avoid SitJump -> JumpForward `SitAvoidFwd` (blend 0.1), speech PedestrianAvoidJump (4). The gate `[ped+5712]+1416 == 0` is not decoded (taken as open). The body plays it (`advance_peds`) while the flag is up.

### Seated peds avoid an approaching skater

(2026-10-24, USER-ASKS 326; research b229, b583; user 2026-10-18: "they get up in reaction to you coming at them".)

- **Problem:** a seated ped only reacted to its seat moving or to a hit; retail peds on a bench flinch or jump up
  when a skater comes at them.
- **Retail [code + trace]:** every ped has a SkaterAvoidance avoider (nav modifier 2, `ped+2832`, vtable 0x8232CA50),
  and sitting does not turn it off. Per sensed mover, about every 1/3 s in the recomp: skip movers farther than 10 m
  (0x820ED57C = 100, squared) or slower than 1 m/s (0x8231A844); the straight-line closest approach (82E2E738, 3D: it
  reproduces the trace's t and miss to the third decimal) must come within 0.75 s (`+0x44`) and 3 m (`+0x38`); a Jump
  when faster than 5 m/s (`+0x4C`) and closer than 1 m (`+0x3C`), else a Step (82E2E890). The direction is the angle
  of the escape vector (ped minus the closest point, flattened) from the ped's facing about up (8296EC98, read in the
  generated code: acos of the normalised dot, 2 pi minus it when cross(facing, escape) points down; the call passes
  the facing from the owner's vt+0x14 as the first vector), with 30 degree (0.5236, `+0x40`) Forward and Back sectors:
  Left is the ped's own left. The escape point must pass a 0.1 m ground probe (82C46460). Values from the `AVOIDK` /
  `AVOIDTEST` lines of a recomp run (25 tests on 5 avoiders, all the same); two traced decisions (Step Right at miss
  2.08 m, Jump Left at miss 0.425 m and 7.8 m/s) follow the rule.
- **Seated outcome [data, `motiongraph_avoid.xml` from `Locomotion.Sit.SittingCyc`]:** Jump Forward `SitAvoidFwd`,
  Left `SitAvoidLeft`, Right `SitAvoidLeft` mirrored (the ped leaves the seat, speech PedestrianAvoidJump 4); Jump Back
  `SitAvoidBack` and every Step (`SitAvoidSmallFront` / `SitAvoidSmallLeft`, mirrored for Right / `SitAvoidSmallBack`)
  keep the seat (ReturnToSit.StillSitting, SitSmall).
- **Ours:** `skate_core::living_world::peds::avoid` (`AvoidRules::RETAIL`, `decide`, `seated`); `think_peds` tests the
  skaters for each seated ped with SkaterAvoidance on, on the ped's own phase every `period`, first qualifying skater
  wins, escape point probed on the navmesh with 0.1 m (`SEAT_PROBE_TOLERANCE`). The seat-moved jump above goes through
  the same table (Jump Forward). A clip that keeps the seat plays and hands back to the sit sequence's held cycle
  (`PedAnimPlayer::remaining_steps`, held with no time limit like the plugin's own sequence); one that leaves the seat
  ends the sequence as the seat-moved jump does. Logs: `PED_AVOID ped=#N skater=i kind dir seated clip leaves_seat t
  miss speed angle`, `PED_AVOID_SKIP ... why=off_ground`, then the existing `PED_SIT_AVOID` / `PED_SIT_AVOID_DONE`.
- **Moddability:** `ped_brain` tuning `skater_avoid_sense`, `skater_avoid_min_speed`, `skater_avoid_look_ahead`,
  `skater_avoid_radius`, `skater_avoid_jump_radius`, `skater_avoid_jump_speed`, `skater_avoid_sector`,
  `skater_avoid_period` (retail values as defaults, restored when the mod goes); the SkaterAvoidance nav modifier a
  plugin or behaviour switches off (ChangeNavModifierSetting index 2) turns it off per ped.
- **Multiplayer:** decided on the ped authority from positions, velocities and the ped's stable serial phase, no
  RNG; clients see the clip through the body sync.
- **Tests:** `peds::avoid::tests` (the two traced decisions as fixtures, the gates, the sides, the seated table),
  `peds::tests::a_seated_step_avoid_returns_to_the_held_sit_cycle`, and the data test
  `living_world_ped_data_loads_from_the_export` (all six clips resolve in all 12 sitting anim sets).
- **NOT RETAIL YET / open:** standing peds' dodges (the same decision with the standing clips; a follow-up, scope
  freeze); which movers never cause a Jump (actor type 6, flag `+0x3937`) and the kind 3 message a Jump sends to the
  mover; whether the values differ per ped type (one run, ped types not named); the recomp's 1/3 s cadence and its
  phase (ours: the ped's serial); whether a ped that jumped off sits down again. Needs a play test.

### Standing peds dodge an approaching skater

2026-10-29 (research `.local/research/b683-ped-dodge-gaps.md`, checked at the source): retail runs the same
SkaterAvoidance decision (`82E2E538` -> `82E2E738` -> `82E2E890`) for every ped, not only seated ones. The motion
graph's `Avoid` state is entered from Walk, Idle, Converse, Shuffle and Strafe, never from ChaseRun
(`motiongraph_*.xml`), so a running ped does not dodge [data]. Jump: `AvoidJumpForward` / `AvoidJumpLeft` / mirrored
for Right / `AvoidJumpBack`, PedestrianAvoidJump 4 on entering; Step: `AvoidStep*` the same way, no speech, waits for
the locomotion clip's branch window (`WaitForBranch`). Template `avoid.xml`: blend 0.1, out at `WillExpire 0.01`.
Entering Avoid stops the channel (`StopChannel`); AvoidComplete sends PedestrianWandering 1; a hit during the dodge
goes to Collision [data].

Ours: `skate_core::living_world::peds::avoid::{standing, may_dodge, replaces, STANDING_CLIPS}`; `think_peds` decides on
the ped's own phase like the seated test (same 0.1 m escape probe `82C46460`), `advance_peds` plays the clip as a
reaction (Jump at once, Step at the branch window), a skater hit replaces it. A pending Step gives way to a Jump, a
pending Jump blocks new decisions (b683 2.4). Event `PedEvent::Avoid` / mod event `ped_avoid` (seated ones too); mod
switch `ped_brain.skater_avoid_standing`. Logs `PED_AVOID ... standing state=..`, `PED_AVOID_DONE moved=..`. The six
clips resolve in all 12 stock sets (data-gated `living_world_ped_data_loads_from_the_export`); core test
`a_standing_ped_jumps_or_steps_from_walk_and_idle_only`. No game-side harness runs ped minds, so the trigger itself
is checked in game.

The Jump's mood post (research `.local/research/b685-skateravoid-post-and-movers.md`, asm checked): after a Jump only,
`sub_82E2E890` posts category 3 (`skateravoid`) into the dodging ped's own store (interface `0x8269A960`, through
`sub_82E3DA40`): position the ped, instigator the mover, second the ped; the two 1.0 floats are vision-test scales,
not the magnitude [code]. Ours: `PedMind::avoid_post`, posted by the mood step on the next think through the same
prerequisites as `greeted` (seated and standing). Only the stock `jock` reaction set lists `skateravoidwarn` /
`skateravoidchase` (count 2 / 3) [data]; data test: three jumps make a jock warn
(`living_world_ped_mood_tables_warn_then_chase_on_collisions`).

Open (b683 / b685): NPC skaters as movers (retail takes any actor the nav sensor's filter bit 0x01 reports; which
actors carry it is not found),
the mover kind / `+0x3937` no-Jump gate, the global byte gate, when retail clears the two latches; whether the seated
avoid also sends AvoidComplete's speech.

### Plugin behaviour end

Leaving the Plugin state, or moving to another plugin graph, first ends the old graph's active behaviours: the Plugin behaviour's End (`8269F310` -> `82E0CFA0`, controller vtable +0x10 = EndAllBehaviours `82C13498`). So the sit body's End removes its packet (Sit2Stand plays) and OwnPluginObject / IgnoreStandingCollisions / LockToCurrentPosition clear as in retail (b300).

### Spawn conversation area

SpawnConversationArea (`826A6CD8`): none within 50 m of another; 3 m ahead (no check there); the starter and its target join. The waypoint generator (`82E1D620`, b134) drops the centre with a +-3 m world ray (`82E166C0`; a miss = no waypoints) and keeps the waypoints only when all land on low-cost navmesh (no roads): an area without waypoints dissolves.

### Intercept chasee

InterceptChasee Update (`826A3BF0`, b306, recomp-checked tail `loc_826A3F08`): every think the mover gets a destination. The chasee position, unless the ped and the chasee are reachable (`82C465A8`) and the solver finds an intercept: then the lead point, on the mesh where it projects (`82C460E0`), else as it is. A swimmer is handed over as it is (no snap onto the bank), so the reachability check below fails. The body's route only follows a destination on the mesh (a path request to an off-mesh point keeps the old path, `82E957F8`).

### Cannot reach chase target

CannotReachChaseTarget (`82E31D58`, b108 / b302): every 10 mover updates the mover checks that its destination and the ped are on the navmesh and connected. An unreachable report rewrites the bit to "within 5 m flat of the chasee"; a reachable one leaves it, so a chasee climbing back onto the mesh does not end the wait.

## Code reference: skate-core peds/wander.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/wander.rs`; each comment there links its section here.

### Ped wandering

Pedestrian wandering (doc 26, peds milestone M3): retail's `NoRoadWander` goal on the navmesh, a mod-only crosswalk rule, ped-to-ped avoidance and the walk / turn intents for the animation player.

What retail does [code, TU3; addresses are evidence only]:
- **The ped graph never takes the road branch.** `Pedestrian.xml`'s `Wander` state picks `WanderMode.Road` (FollowRoad + `UseCrossWalk`) while `HasRoadWanderTarget`, and goes to `WanderOnRoad` on `IsOnRoad`; in `StateGraph::TheConditionFactory` (`sub_826C1730`) both names are registered with the generic factory `sub_82BC3F68` (`sub_82F7A698`, `sub_82F79698`), whose evaluate slot (vtable `0x8231ED5C` + 48) is `sub_8274CA90` = return 0. So every ped runs `WanderMode.NoRoad`: `CheckForRoadTarget` (all slots no-ops) and `NoRoadWander` (`sub_826A2FB8`). The crosswalk states and `WalkSignSaysGo` (`sub_826AC190`) are unreachable from the ambient ped graph. (The real `IsOnRoad` / `IsNearCrossWalk`, `sub_826C48F0` / `sub_826C4878`, live in `BehaviourGraph::TheTransferConditionFactory` for plugin entry rules such as `usetrashbin.xml`.)
- `NoRoadWander` (and `Wander`) switch the ped to its NavPower mover (`sub_82E360F0(1)`: `ped+2224`) in mode 6 (`sub_82E35E80`): the mover's goal becomes the wander goal at `ped+5632` (vtable `0x8232C9C8`), its value `+12` = 2.0 (`0x82060C50`).
- **Target choice** (`sub_82E30F58`): direction = the ped's forward vector (owner vfunc 20), or, after event 4, the unit vector from the old target to the ped. Unless an event 1-3 came in, probe 5 extra directions within 90 degrees at 40 m (`0x821EB794`, `0x82256FD8`); if none fits, 9 within 270 degrees at 10 m (`0x821963E4`, `0x82063B20`); if none fits either, the target is `position + direction x max(3, 1.1 x value)` (`0x82063B08`, `0x82099148`).
- **The probe** (`sub_82E311A8`): straight ahead first, then probe `i = 1..=n` at angle `ceil(i / 2) x (spread / 2) / (n / 2)` (n / 2 rounded down), negative for odd `i` (`0x8216DEE0` = -1); a direction fits when its end point is on the navmesh and reachable from the ped's polygon (`sub_82C464F0`). The fitting end point becomes the target.
- **Re-targeting** (`sub_82E2CB08`, mover vfunc 44): when the NavPower bot reports state 1 the goal picks a new target from the ped's position and NavPower is sent there.
- Speed: `SuggestVelocity linear=3.0` in `Wander` [data]; the walk itself is root motion (walk clip 1.325 m/s, recomp walking median 1.30 [trace]).

Ours (stated): NavPower's path following and local avoidance are not decoded. A ped follows the funnel corners of [`NavMesh::find_path`], steers toward the next corner at [`WanderParams::steer_rate`] (measured 255 deg/s) below the first turn edge and posts retail's turn buckets past it (`82E2A500`: 45 / 90 / 135 / 180, standing or walking, the clip turns the ped; the angle is to the next corner, retail's against NavPower's steering velocity `82E2ABF8` / `82E2AAA0`, b169), re-targets on arrival (the bot's state 1) and, when a path cannot be found, as after an event 1-3 (short fan next). Avoidance: a ped yields (stands) while a ped with a lower id is inside its personal space ahead of it, side-steps a ped with a higher id standing in its way, steers away from close peds, and never takes a step that brings it within twice the agent radius of another ped ([`separation_ok`]). The skater is not avoided by navigation (retail peds walk into a standing skater, PEDCOLL; the motion graph's `Avoid` reactions come with the behaviour milestones).

### Ped steer rate

The walking steer rate, radians per second: 255 deg/s [trace, recomp PEDFACE at PCU Library and Aletown: 30 peds, 9136 walking samples, the facing follows NavPower's steering direction with no lag and its corrections outside turn clips peak at 257 deg/s, p99 149]. Code candidate (b158, meaning unverified): the bot value +0x80 = clamp(255 x max(1, s), 90, 720) of the truncated speed (`82E2D260`), 255 at s = 1; the 510 it gives at the wander speed 2.0 is never seen in the trace.

### Ped still re-path

A walking ped that stays within `stuck_distance` (0.13 m) for `stuck_seconds` (0.5 s) re-paths to the SAME target, once per still spell. The values are the bot's still timer (`82E97118`, b313 / b314), but retail runs that timer only in bot mode 4, not for walking peds (b336): on walking peds this re-path is OURS. The outcome matches retail, which never gives a walking ped's target up for lack of progress (b190, b332).

### Ped yield patience

NOT RETAIL (ours): seconds a yielding ped waits before it re-paths to the same target. Keeping the target is retail's outcome; the 2.0 s is borrowed from the bot's mode-0 (no path) controller `82EA4408`, which walking path peds never run (b335). Retail's walking path step `82EA0808` has no neighbour handling at all (b336): our yield, sidestep and push are ours; what keeps retail walking peds apart is not read yet.

2026-10-29 (research `.local/research/b689-ped-pathing-gaps.md`, gates checked in the recomp asm): the candidate is
the Pedestrian nav modifier's sensor `sub_82E2D3F8` (ped+2704, vtable `0x8232BA8C`, b337). Its classifier
`sub_82E2D4E0` acts only within 3.0 m of height (`0x82063B08`); a mover faster than 5 m/s (speed squared against
25.0, `0x8209994C`) goes through `sub_82E2D660` to the same Jump / Step pair as SkaterAvoidance (`sub_82E2DDB8`); a
slower one through `sub_82E2D7B8` to hold (`+0x74` bits 0xA0, `+0x20` zeroed) or a steer-around point (`sub_82E2DFB8`,
bits 0xC0, a -10 degree turn scaled 3.0 at `+0x10`) [code]. So walking peds would never jump for each other. Not
ported yet: what reads `+0x74` / `+0x10` / `+0x20` (how long a hold lasts, how far the steer bends) and whether peds
report each other to this sensor at all need one recomp trace (hooks listed in b689). The gate itself is ported:
with the modifier off the nav step gets no neighbours (`peds.rs`, `avoid_peds`).

## Code reference: skate-core peds/obstacles.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/obstacles.rs`; each comment there links its section here.

### Ped navigation obstacles

Dynamic objects as navigation obstacles for peds (doc 26, fix 11): props (DMOs) and mod bodies cut out of the walkable area while they rest, so peds plan their paths round them.

What retail does [code, TU3; addresses are evidence only]:
- Every world object has an obstacle interface on its vtable: register `sub_82595140` (via the base slot `sub_82595010`, vtable + 4) and update `sub_82595298`. Slot +68 says whether the object is an obstacle at all. It returns 1 for the DynamicObject class (vtable `0x82322D50`, the string `dynamicobject` sits at `0x82322D2C`), the player skater (`0x82300910`) and one more class (`0x823220E0`); peds (`0x8232BE80`) and the other actor classes return 0.
- A DynamicObject gets its obstacle record from the pool `DynamicObject Obstacle MemStore` (slot +76 `sub_82C57268`, record ctor `sub_82C46778`, record vtable `0x82322D10`). The box comes from its collision body (slot +96 `sub_82C486F8` -> body vfunc 160 = half extents; slot +92 `sub_82C48688` = orientation from the collision shape). Slot +88 `sub_82C48648` switches the obstacle off while the object's state word `+144+4252` equals 1. That word is set to 1 only by the component's slot 32 (`sub_82C56B00`, which also moves every collision element of the object to collision group 13; slots 33/34 set it back to 0 with group 12 or 14). It means "attached to a ped's hand" (b349: slot 32 is the hand attach, slot 34 `82C56C70` the release that `82E3EBE0` calls for a drop or throw), so a ped's hand prop is no obstacle while held and a normal cut from its release on. It is NOT the Move Object hold: the hold's keep-alive (interface slot 10 -> DMO handler slot 6 `sub_82C4C450` -> component slot 30 `sub_82C485D0`) only sets the held bit `DMO+4464 & 0x20`, which the obstacle code never reads. So a held prop stays an obstacle: cut while slower than 0.4 m/s, a moving avoider while faster.
- Placement `sub_82C477B0`, called on every update: half extents below 0.2 m are raised to 0.2 (`0x82099280`); while the object moves faster than 0.4 m/s (`0x82181B90`, slot +24 = velocity) its cut is removed and NavPower's moving avoider (`+64`, `sub_82E99998`) takes over; at rest the box is cut into the NavPower mesh (`sub_8293EF28` with the record `{-1, 15.0, type 4}`, then `sub_8293F340`), and re-cut only after the object moved more than 0.25 (`0x820C6D98`) x its smallest half extent from where it was cut.
- NavPower then plans every bot's path on the cut mesh (`dynAreas` in its planner), so a resting prop is walked round, a prop the player moved counts where it lies now, and a rolling prop (or a held one being moved faster than 0.4 m/s) is not part of the mesh but a NavPower moving avoider.

Ours (stated): NavPower's polygon cutting and its moving avoider are not decoded. A cut is an oriented rectangle in xz (the box projected onto the ground); a path leg that crosses a cut (grown by the agent radius plus [`ObstacleParams::detour_margin`]) gets a detour corner at the cheapest free rectangle corner on the mesh; a target inside a cut does not fit (retail: its end point no longer snaps to the cut mesh). Whether the record's `-1` / 15.0 mean "blocks every mover" / "penalty x15" is not decoded; cuts block here. Bodies (cut or moving) are also solid for a ped's step ([`NavObstacles::step_ok`]), standing in for the physical contact.

Deterministic: obstacles are kept by stable id in a `BTreeMap`, the grid is a `BTreeMap`, ties are broken by id; [`NavObstacles::version`] changes only when a cut changes, so nothing is rebuilt per tick while props rest.

### Moving obstacle solid

A moving (uncut) object is solid for a ped's step ([`NavObstacles::step_ok`]). NOT RETAIL YET: retail hands a moving object to NavPower's moving avoider instead (`sub_82E99998`: an 88-byte record in the planner's obstacle database, position and velocity refreshed every tick by `sub_82E998C8`, radius = 0.35 x a planner-wide value, independent of the box); how NavPower's bots steer round it is middleware internals, not decoded. `false` = moving objects do not block a ped's step (the switch a decoded avoider port would replace).

### Contact slide

Problem (DownTown session 2026-10-29): a fleeing ped met the player's standing skater head-on and stood pressed against it for 45 s. The face clamp of [`NavObstacles::resolve_step`] keeps only the part of the step along the face; head-on that part is zero, so the ped never moved.

Retail [code, TU3, research b698, re-read 2026-10-30]: the path controller's steering (`82E9FD78`) adds, for every moving avoider the ped touches (`0 < dist < r_sum`), a push-out along `n = (p_ped - p_avoider) / dist` and, from `82E9D0C0`, a tangential slide `perp(n) * 0.7 * |push|` (`0x821161AC`). The slide applies when the ped heads into the body, `dot(dir, n) < -0.3` (`0x822F862C`), and the avoider's speed along `dir` is below 0.5 x the ped's max speed (`0x8209975C`). `dir` is the goal direction while the ped is slower than half its max speed, else its velocity direction (the 0.5 test only picks `dir`; b698 read it as a second trigger). The side is the one `dir` already leans to (sign of `cross(n, dir)`); dead centre takes a fixed side of `n`.

Ours: in the step resolver, for a moving (uncut) body only, the step is replaced by a tangential step of `(|dot(d, t)| + 0.7 x |dot(d, n)|) x step length`, the push cancelling the inward part as in retail's sum; the result is still face-clamped and step-checked. Cut props keep the face clamp (retail routes round cuts on the mesh). Mod keys `ped_obstacles.contact_slide` (0.7, 0 = off) and `contact_slide_dot` (-0.3). Deterministic (no RNG; first blocker in id order).

Speed tests (research b717 / b718, main re-read the recomp's compare `skate3_recomp.100.cpp` 24926-24937 / 25077-25081 and the constant 0x8209975C = 0.5, 2026-10-30): `S` = the ped's path speed (`settings+0` x agent `+0x14C` x `+0x144`; 2.0 m/s for walking peds in the trace, 1.0 on some legs); `dir` = the unit direction to the steer target (`ctrl+0x30`) when the ped's speed (a true length) is at most `0.5 x S`, else its velocity direction; the slide needs `dot(dir, n) < -0.3` and the avoider's own velocity along `dir` below `0.5 x S` (strict). When that fails the ped is only pushed out (no slide, no stop in this function). The slide only runs while the ped follows a path (controller mode 2); a held ped (mode 3) gets the push and then brakes. Ours: [`NavObstacles::resolve_step_contact`] with [`Contact`] (the ped's velocity and next corner, `peds.rs`); without it the step direction stands in. Mod keys `ped_obstacles.contact_slow_fraction` (0.5) and `contact_max_speed` (2.0).

Open: run / flee path speeds (not traced, so 2.0 for every ped); the mode 3 hold is not modelled. The dead-centre side assumes retail (x, y) maps to our (x, -z).

Tests: `obstacle_tests::a_ped_running_head_on_into_a_skater_slides_round_it`, `the_contact_slide_tests_the_avoiders_speed_and_a_slow_peds_steer_target`.

### Predictive side-step

Retail [code, TU3, research b698 / b699; constants dumped from the image 2026-10-30]: before contact, the path controller (`82E9FD78`) adds a sideways acceleration for the moving avoider with the nearest predicted contact. Pick (`82E9CE90`): relative velocity `dv = v_av - v_ped`; a ray from the avoider along `dv` against `0.99` (`0x820ED5E8`) x the combined radius round the ped gives the contact time `t`; kept when `0 <= t < 3.0` s (`0x82063B08`), strictly lowest first (id order), never for bodies already overlapping. Force (`82E9CBD0`): `s x 3.0 x 1.3` along the left perpendicular of the ped's velocity, 1.3 being the NavPower global scale (`0x82099484`, written by `8292E230`); skater and prop avoiders weigh FLT_MAX (`82E99998`), so the weight factor is 1. Side `s`: head-on (velocity dot < -0.7, `0x822F8F80`) or a standing avoider -> away from the avoider's predicted position as seen from the ped's current one, dead centre left; crossing (dot <= 0.7) -> by the side of the avoider's velocity (the ped never has right of way over a skater or prop); same direction -> by comparing both predicted positions. Skipped while the ped brakes for its path end (`82E9FC20`).

Ours ([`NavObstacles::side_step`], used by `PedNav::step_avoiding`): the ped's velocity is its last step over `dt`; retail's (x, y) is our (x, -z), so retail's left perpendicular `(-u.y, u.x)` is our `(u.z, -u.x)` (the ped's left, facing +z). The acceleration builds a lateral velocity that adds to the path direction; the steering then turns toward that sum. Retail's path turn term pulls the lateral velocity back every tick (below). A ped with SkaterAvoidance off (chasers) ignores skaters here as in the contact step. Mod keys `ped_obstacles.avoid_horizon` (3.0, 0 = off), `avoid_ray_scale` (0.99), `avoid_accel` (3.0), `avoid_scale` (1.3), `avoid_head_on_dot` (-0.7), `avoid_crossing_dot` (0.7). Deterministic: no RNG, id order.

Avoider radius (b699 round 2, `82C477B0` re-read 2026-10-30): retail gives every moving avoider the extents `{a, 0.2a, 0.2a}` with `a = 1.3 x 0.25` (`0x820C6D98`, `0x82099280`), so 0.455 m whatever the box; keys `avoider_extent` (0.25) and `avoider_extent_tail` (0.2), times `avoid_scale`. The third extent is the research agent's reading of the vector code.

Pull-back (b699 round 2, constants dumped 2026-10-30): retail sums the steering terms of `82E9FD78` without an acceleration clamp; `82EA0808` integrates `v += a dt` and clamps the speed. The velocity returns to the path through the path-turn term `82E9DF70` (lateral `perp x min(angle / 5 deg, 1) x A_turn`, `0x822F9374` = 11.459 per radian; boosted above 10 deg, `0x822F9368` / `0x822F936C` = 4.5837; only below 120 deg, `0x822F9370`), not a separate fade. Runtime values (research b716, the skate3recomp hooks PEDSET / PEDTURN, `.local/research/b705/fire_trace.tsv`, main re-checked the trace): every walking ped has `A_turn` (`settings+0x2C`) 40 m/s^2 and `settings+0x08` 0.35; `82E9DF70` returns an acceleration (speed term plus turn term) whose turn is capped by the sideways speed over the controller step `f1` (1 / 30 s) and by the centripetal limit `|v|^2 / (2 x 0.35)`: 308 of 6788 at-speed samples sit exactly on that limit (5.714 at 2 m/s), 8 above it. So the turn removes the side-step's lateral velocity within one controller step and it settles near `a x f1` (about 0.13 m/s at 3.9 m/s^2) while an avoider is predicted. Ours: [`NavObstacles::path_turn`] runs every tick after the side-step's push (`PedNav::step_avoiding`), replacing the fade; our 60 Hz tick halves the lateral velocity per tick when nothing pushes (gone in about 0.1 s, retail in one 1 / 30 s step). The corner and target-approach boosts of the same term are left out (our path follower turns corners itself). Mod keys `ped_obstacles.path_turn_accel` (40), `path_turn_full_deg` (5), `path_turn_boost_deg` (10), `path_turn_boost_rate` (4.5837), `path_turn_boost_max` (5), `path_turn_max_deg` (120), `path_turn_radius` (0.35), `controller_step` (1 / 30). The crossing give-way gives zero force (recomp `sub_82E9CBD0` zeroes the output first); it never applies to skaters or props.

Braking (`82E9FC20` / `82E9F9F0`, research b716): within the goal radius of its steer target (retail sets it per path leg: 2.0, 0.75 and 0.31 m seen) a ped brakes when a moving body ahead along its velocity is closer than the stopping distance `v^2 / (2 x 80)` (a ray against 1.225 m plus the body's avoider extents; bodies moving away at a quarter of the ped's speed or more are left out); braking skips the side-step and stops the ped within one step. Ours: [`NavObstacles::brakes`] in `PedNav::step_avoiding` (2.0 m goal radius for every leg, the stop through the yield); keys `ped_obstacles.brake_decel` (80, 0 = off), `brake_goal_radius` (2.0), `brake_body_radius` (1.225), `brake_away_fraction` (0.25). At walking speed it fires only within about 2.5 cm of contact.

Open: run / flee speeds are not traced (a scripted dumpster push touched the dumpster but made no ped flee, 2026-10-30; the props trace preset now also logs `pedmove` for a played session); the Flee avoid mask (b135 open 3) is not read, so fleeing peds side-step like walking ones.

Tests: `obstacle_tests::a_ped_side_steps_a_skater_up_to_three_seconds_ahead`, `the_path_turn_holds_a_side_step_near_one_controller_step_of_push`, `a_ped_brakes_only_for_a_body_inside_its_stopping_distance_near_its_goal`.

## Code reference: living_world/peds/nav.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/nav.rs`; each comment there links its section here.

### Ped navmesh

The pedestrian navmesh (doc 26, peds milestone M3): retail's NavPower nav graphs as polygons with neighbours, point location, reachability and polygon paths.

Retail [code, TU3; addresses are evidence only]: every ped owns a NavPower bot (`ped+5940`) that its mover (`ped+2224`, vtable `0x8232C708`) drives: the mover hands the bot a destination (`sub_82C47378`) and reads its steering direction back (`sub_82C47198`). The wander goal probes destinations with a nearest-polygon search (`sub_82C46348`, radius 0.5 `0x8209975C`) and a reachability test (`sub_82C464F0` -> `sub_82926BA8`, a per-graph connectivity bitset), not with a straight ray. The polygons come from the `0x00EB0027` objects (`tools/asset_pipeline/living_world_navmesh.py` documents the layout) [data].

What is ours (stated simplifications): NavPower's path search and path following are not decoded, so a path is an A* over polygon neighbours (portal midpoints, equal cost per metre unless [`NavRules::area_cost`] says otherwise) smoothed by the funnel algorithm; reachability is a connected-component label; polygons of [`NavRules::blocked_areas`] (default the 0xF1 area, where no recomp ped position ever lies [trace]) are not walkable.

Tile seams (fix 17): NavPower tiles stop about 0.12 m short of their shared border [data] and setup links the two sides, sometimes on one side only; [`NavMesh::move_along`] steps across the gap over linked polygons only (never onto an unconnected wall-top layer), and the links are used in both directions.

Pure and deterministic: no hash-map iteration, ties broken by polygon index, so every machine that loads the same mesh plans the same paths.

### Navmesh locate

The walkable polygon under `p` (closest in height within [`NavRules::locate_height`]), or the closest walkable polygon edge point within [`NavRules::locate_radius`]. A polygon under `p` more than the agent's step height (the agent block's third value, 0.2 [data]) above or below it competes with the edge points by vertical plus horizontal distance, so a point in the gap of a tile seam lands on its own layer's edge, not on a wall-top layer overhead (fix 17).

### Navmesh locate nearest

The point nearest to `p` (3D) on the polygons reachable from `from`, within `max_dist`: retail's path goal search (`82933E18` -> `82933A48`, best distance starts at `0x822F9310` ~9.2e18 and the query range is `0x8206D108` FLT_MAX, so unbounded; only the start's region set, no area test; the closest point of each polygon by `8292F9F0`; b277). Inside a polygon's outline the point is its surface under `p` [ours: vertical, retail projects onto the plane]. Ties go to the lower polygon index (deterministic).

### Navmesh start search

The nearest walkable polygon point to `p` (3D), any component, within `max_dist`: retail's path start search (`82933848` -> `829338B0`: range FLT_MAX, best distance from ~9.2e18, the closest point of each candidate by `8292F9F0`; b324), for a start [`Self::locate`] refuses (more than 0.5 m off the mesh). Inside a polygon's outline the point is its surface under `p`, else its nearest edge point. Searched ring by ring over the grid from `p`'s cell, stopping once no further ring can hold a nearer point; ties go to the lower polygon index.

### Navmesh move along

Move from `from` toward `to` over the surface: only polygons linked to `from.poly` (directly or through linked neighbours near the move) can take the step, so a step never jumps onto an unconnected layer (a wall top or planter island above the ped), and a step into the gap between two tile-stitched polygons is kept (the tiles stop short of their shared border, about 0.12 to 0.14 m each side [data]) instead of being snapped back to the edge it left. Against a boundary edge the step slides onto the edge (as [`Self::locate`]'s snap). Retail: the NavPower bot moves over linked polygons only (path follow not decoded; fix 17).

## Code reference: living_world/peds/anim.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/anim.rs`; each comment there links its section here.

### Ped animation player

Ped animation player for the 50-bone ped rig (`PedestrianSkeletonPres.abin`).

What it ports (tags as in doc 26):
- clips are additive over the bank's `PEDESTRIAN_RIG_TPOSE` pose record and carry only the first 6 rig parts (bones 0..=26: trajectory, torso, head, arms, legs, prop); fingers, face and twist helpers have no data in the bank [data]. Final local = `AddSQT(clip, reference)` like the skater's `AddBindPose` (`pose_add`, `0x828CCC08`).
- sampling: `select_frames` / `sample_key` (`0x82D20788`), length `(frames - 1) / fps` (`clip_clock`), the trajectory delta with the clip's loop transform on a wrap (`pose_trajectory::delta`, `0x82D178D8`): that delta is the root motion.
- logical names through `livingworld_entity_animation` (`PlayRemappedAnimation`): an entry is one clip or a list (uniform pick); `tAnimAttributes` entries add branch windows [data].
- the locomotion part of `MotionGraph_Pedestrian.xml` M2 needs [data]: `IdleBasicCyc` (blendTime 0.1, `cycleSwappingPercentage` 0.5), `Stand2Walk` (0.25, leaves 0.03 s before its end), `FwdWalkCyc` (0.1), `Walk2Stand` (0.15, leaves 0.1 s before its end, entered inside a walk branch window), `StandTurnR180` (0.15, mirrored for the left turn).
- mirror: `pose_mirror::mirror` (`0x828CDAF8`, trajectory mode 1) with the rig's partner table, on the full pose (delta + reference); the root motion is reflected on x.
- foot plants and body falls: clip attributes `LEFTTOEDOWN` / `RIGHTTOEDOWN` / `BODYFALLTYPE`, phase windows x length (`playback_clip` attribute status) [data].

Simplifications until the behaviour runtime (M4) runs the real motion graph: the crossfade weight is linear over `blendTime` (retail's transition curve is not read); the cycle swap is a uniform draw at each wrap; the intent (idle / walk / turn) comes from [`TestPath`] or the caller, not from the AI graph.

### Knockdown contact

A contact on a knocked-down ped ([code] `sub_82E38FB8` sets `c+8` unless `c+9`; `sub_8269E058` / `sub_8269E170` consume it). Down: it waits for the timer's expiry, which then re-arms the timer. Getting up: a re-hit whose posture comes from the get-up clip's time (`sub_8269E460`); LyingDown keeps the stored direction, Crouched / Standing take `direction` and reset the expiry count.

### Ground time

[data] `WillExpire InTime=0.01` ends a clip step; the ground cycle runs until the AI's `Recover` (the set's ground time); then back to locomotion (idle). The ground time counts from the reaction's Begin, not from the cycle (b372: `sub_8269DA00` starts it with the fall, `sub_8269E058` posts Recover at expiry), so the get-up comes at the later of the fall's end and `ground_seconds` after the hit.

### Ped pose and mirror

The local pose (one SQT per rig bone, reference added, trajectory zeroed) at the player's time plus `ahead` seconds (render interpolation; no state change).

Each layer is made a full pose (clip delta added onto the reference) BEFORE it is mirrored: `pose_mirror::mirror` with trajectory mode 1 is a full-pose operation (it multiplies the root's children by the literal 180 degree quaternion that the rig's reference hips carry), so mirroring a bare delta turned the whole body upside down in the mirrored left turn. The reference is symmetric under it (the export: every bone within 7 degrees), so a zero delta mirrors to the reference. Blending the full poses equals blending the deltas and then adding (the add is a left multiply / affine map, which nlerp and lerp commute with).

## Code reference: living_world/peds/mod.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/mod.rs`; each comment there links its section here.

### Ped core

Pedestrian body (doc 26, peds milestone M2): which entity / model a census spawn becomes, the ped animation player and the foot events the audio reads; navigation (milestone M3): the NavPower navmesh (`nav`), retail's wander goal and avoidance (`wander`), the mod crosswalk rule's walk lights (`crosswalk`). Pure and engine-independent (no ECS, no I/O); `skate-data::ped_anim` fills the data types from the user's export, the game (`skate-game::living_world::peds`) hosts it.

Retail reference (TU3, addresses are evidence only):
- entity inside the census category: `sub_826B8B88` after the category roll; the draw is `sub_826BB058` (world RNG u32 x 2^-32, `0x822F88F4`), the index `trunc(draw x 100) % entities.len()` (`0x820ED57C` = 100) [code];
- model tints: `sub_827B4170`: one rand `r`, `secondary_colours[r % na]` (`tints_a`) and `chassis_colours[r % nb]` (`tints_b`) [code]; the ped shaders recolour the atlas's mask texels with them (`colorize`) [code, shader];
- clips: `PedestrianSkeletonPres.abin`, additive over its `PEDESTRIAN_RIG_TPOSE` pose record (the clips hold 6 of the rig's 10 parts: bones 0..=26) [data];
- foot plants: the clips' `LEFTTOEDOWN` / `RIGHTTOEDOWN` attributes (phase windows), the body falls `BODYFALLTYPE` (value windows) [data]; retail's audio reads them as `S+74` / `S+73` and `S+76` (`world-ped-audio.md`).

Multiplayer: a ped's look is a pure function of its spawn record (category + seed) and the catalog; its animation steps once per world tick from that seed, so the state follows from the spawn record and the tick.

## Code reference: living_world/peds/fade.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/fade.rs`; each comment there links its section here.

### Ped draw fade

Ped draw fade: retail never shows a ped pop at the census cull because the ped is already drawn fully transparent by then.

Retail reference (TU3, addresses are evidence only):
- `sub_827C1188` (per living-world render instance, every frame) [code]: the model record's pair `Hash_73B6874C7B46C7C6` (near, far) (peds 45 / 55 m [data]) against the distance from the camera: opacity `1 - clamp((d - near) / (far - near), 0, 1)` beyond `near`, 1 inside. The pair `Hash_9FCFDBEA56BA4733` (65 / 75) replaces it only when its third float is larger (`sub_827C1870` lerp); both are 0 for every ped model [data], so 45 / 55 applies.
- The same function keeps a spawn fade in at `+576`: `+= 1/30` per console frame (`0x8232BA4C`), clamped to 1, so about 1 s at the console's 30 fps; the drawn opacity is the smaller of the two, and below 1 the instance draws blended (`+580`).
- The census cull (`sub_826BA8B0`, 70 m slow / 90 m fast around the focused skater) is a pure 3-D distance test with no camera or visibility rule; only security guards hand the "beyond" flag to their `ISecurityGuard` component (vfunc +4). So the fade above is what hides the cull from the player.

Moddable: the distance pair comes from the model record (a mod model sets its own), the defaults and the fade in time are plain data in [`PedFadeConfig`].

## Code reference: living_world/peds/colorize.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/colorize.rs`; each comment there links its section here.

### Ped colorize

Ped clothing colours: retail's ped pixel shaders recolour the mask texels of the diffuse atlas with the model's tint pair (doc 26, "Ped clothes drawn in mask colours").

Retail (TU3 `shaders_final.big`, `livingworld_stamp_defaultPS` for material type `pedestrian_high_stamp`, `defaultlivingworld_defaultPS` for `pedestrian_low`; both read the same way) [code, shader microcode]:
- `lin = diffuse.rgb^2` (the shaders light in squared space and write `sqrt` at the end);
- a texel is a mask texel when `G^2 < 0.001225` (a shader literal, green under 0.035);
- a mask texel becomes `R^2 x i_colorize_red + B^2 x i_colorize_blue`, any other keeps `lin`.

`sub_827B4170` picks the pair from one rand: first `secondary_colours[r % n]` (stored at +80 of the ped's presentation slot), then `chassis_colours[r % m]` (+96) [code]. Which constant each feeds is from the data, not from a traced upload: the base model record holds secondary `(1, 0, 0)` and chassis `(0, 0, 1)`, the pair that turns the rule into the identity only as secondary -> `i_colorize_red`, chassis -> `i_colorize_blue` [data]. The constants are taken as the table values (the CPU upload path is not traced).

Our renderer has no ped shader of its own: the game bakes the rule into a copy of the diffuse texture per tint pair ([`colorize_rgba8`]); `sqrt(lin)` is stored back in the sRGB texture, which the GPU's sRGB decode turns into about `lin` again (retail's squared space approximates the same curve).

## Code reference: living_world/peds/choice.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/choice.rs`; each comment there links its section here.

### Ped choice

Which ped a census spawn becomes: entity inside the category, its model (look), tints.

Retail order (TU3): the census picks the category (`sub_826B8B88`, ported in `census::pick_category`), then in the same function the entity: count = the category's `entities` array (`Hash_D5E1267E2D715124`, `sub_8269B040`), draw `sub_826BB058` (u32 x 2^-32 as f32), index = `trunc(draw x 100) % count` (unsigned remainder), count 0 = no spawn [code]. The model's tints come from one rand `r` (`sub_827B4170`): `tints_a[r % na]`, `tints_b[r % nb]`, a default vector when an array is empty [code]. `tints_a` is the model's `secondary_colours` list (`Hash_DF76D7D773857EDB`, read first), `tints_b` its `chassis_colours` (`Hash_12026E2EED18CC8D`) [code: the two hash keys in `sub_827B4170`]; the ped shaders paint the atlas's red mask with `tints_a` and the blue mask with `tints_b` (`colorize`).

Our draws come from the spawn record's seed (one sub-RNG per ped, fixed draw order: entity, group child, tints), not from retail's global RNG, so a ped's look is a pure function of its spawn record. The stream differs from retail's; the distribution is the same.

Group model records without a recipe (`skater_female`, `skater_male`, `worker_male` [data]): the looks are the records that inherit from them. How retail picks among them is not read from the code yet: placeholder = a uniform pick by the seed (`draw % children`), children in key order. Parked in doc 26.

## Code reference: ped_anim.rs

Detail moved out of the code comments of `crates/skate-data/src/ped_anim.rs`; each comment there links its section here.

### Ped anim data

Pedestrian body data (doc 26, peds milestone M2): the ped animation bank and the look / animation tables, read into the pure `skate_core::living_world::peds` types.

- Bank: `data/anim/PedestrianSkeletonPres.abin` from the user's stock data (462 VBR clips, one 50-bone rig with trajectory, 10 parts) [data]. Its clips carry only the first 6 parts (bones 0..=26), so the skater's full-hierarchy loader (`animation_frames`) does not apply; this module decodes the parts a clip has with the same core VBR decoder and leaves the other bones at identity. The reference pose is the bank's `PEDESTRIAN_RIG_TPOSE` pose record. Clip attributes (`LEFTTOEDOWN`, `RIGHTTOEDOWN`, `LEFTHEELDOWN`, `RIGHTHEELDOWN`, `BODYFALLTYPE`, ...) come from `animation_metadata` (first payload word = the value).
- Tables (`private/living_world/tables.json`): `livingworld_entitycategories.entities`, `livingworld_entities` (`model`, the `livingworld_entity_animation` ref), `livingworld_models` (`recipe`, `voice`, `tints_a` / `tints_b` = `secondary_colours` / `chassis_colours`, distance pairs) and the animation sets, whose logical names are vault hashes (`attrib_hash::hash("FwdWalkCyc")` = `Hash_4AA12E0083F10739`) and whose `tAnimAttributes` entries name their clip through `anim_name` (setup resolves the string-pool offset, `living_world_anim.py`).

## Code reference: ped_nav.rs

Detail moved out of the code comments of `crates/skate-data/src/ped_nav.rs`; each comment there links its section here.

### Navmesh file

Pedestrian navmesh data (doc 26, peds milestone M3): `private/living_world/navmesh.bin`, the retail NavPower nav graphs (`0x00EB0027`) that setup decodes and joins across tiles (`tools/asset_pipeline/living_world_navmesh.py` documents both formats), read into `skate_core::living_world::peds::NavMeshInput`.

`navmesh.bin` (little-endian): `LWNAVMSH`, u32 version 1, u32 district count; per district: u8 name length + ASCII, f32 x4 agent parameters, u32 vertex count + f32 x3 each, u32 polygon count, per polygon: u32 first vertex, u16 vertex count, u8 area, u8 tile-stitched edge count, u32 flags, then i32 neighbour per edge (-1 none). A mod map writes the same file with [`write`] (or fills `NavMeshInput` directly).

## Code reference: living_world/peds/crosswalk.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/peds/crosswalk.rs`; each comment there links its section here.

### Crosswalk walk lights

Walk lights for the mod crosswalk rule ([`super::wander::CrosswalkRule::WalkSignal`]): which signalled junction arm a ped is about to cross and the walk light the shared [`SignalClock`] shows for it.

Retail never consults this for ambient peds (the road branch of the ped graph is unreachable, `wander.rs`). The mapping is ours: a crossing over the arm at node end `e` runs parallel to the traffic of the neighbouring ends, so it uses the walk light of end `(e + 1) % 4` (the walk list of a controller is green inside its car green, `signals.rs`). A mod can supply its own [`WalkSignals`] instead.

## Code reference: skate-game/src/living_world/peds_tests.rs

Test descriptions moved out of the code comments of `crates/skate-game/src/living_world/peds_tests.rs`; each test there links its section here.

### Ped pose dump

Visual check helper (no window): with `SKATE_PED_POSE_DUMP=<file.json>` and the data roots, write native model-space matrices (all 50 rig bones, column-major) for `tools/claude-skills/living-world/tools/render_glb.py --pose`: for the `default` set and up to two more sets (with a model each), poses mid-blend into start / walk / stop / idle / both turns, mid walk, mid stop, idle and mid right / mirrored left turn. Each pose carries its label, set and model.

## Code reference: skate-data/tests/ped_nav_data.rs

Test descriptions moved out of the code comments of `crates/skate-data/tests/ped_nav_data.rs`; each test there links its section here.

### Navmesh data tests

Data-gated checks of the ped navmesh (peds milestone M3) on the user's own export. Skips (passes with a note) without data. `SKATE3_ASSET_ROOT`: one or more asset roots joined like PATH; reads `private/living_world/navmesh.bin` (+ `roads.bin` and `tables.json` for the road and crosswalk checks).

Expected values are the shipped DownTown NavPower graphs as decoded for M3 (asserted here, not embedded in the engine): 36443 polygons (24991 area 0x11, 8997 area 0xA1, 2455 area 0xF1).
