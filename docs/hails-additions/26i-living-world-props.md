# 26i: Living world: Props

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Props pulled toward the player, 2026-10-05

**Problem.** First play video, second map, Aletown spawn (about 1:00 to 1:10): running past them on foot, props
"magnetize" to the player. A trash bin trails the runner at a fixed offset and a newspaper box slides along the wall
with them. Earlier notes said grabbing objects "doesn't appear to work yet".

**Root cause.** In #15's carry glue (`physics/prop_carry.rs`, wired in `physics/frame.rs`), a rising edge of **A**
toggles a grab of the nearest prop within 2 m in any direction. But A is the retail on-foot sprint button: the
derived controller's held timer for action 80 (slot 20) is what publishes `OB_Sprint` (8259AA8C,
`input/offboard_intentions.rs`) [code]. #15's comment says B sprints, but B (action 81, slot 21) only blocks sprint.
So every sprint press near a prop grabbed it, and the drag (0.9 m hold, up to 4 m/s, auto-drop beyond 3.5 m)
pulled it along until the runner outran it. The bug is in #15's original code; its author should hear about it.

**Evidence.** Frames of the video at 0:59 to 1:03 (4 fps): the carry HUD diamond turns white (prop in reach) and
then cyan (carrying) while the bin follows. The bin is left behind once the runner is faster than the drag cap.
[trace: video] Retail code: the on-foot grab-object decision (82D324B0, `physics/biped_ground/grab.rs`) only runs
while Processed2476 bit 22 (GrabWorld) is set. The input listener emits GrabWorld while raw flag bit 28 (action 73,
pad slot 9, RB) is held (8259AF68, `input/riding_intentions.rs`) [code]. So retail grabs with a held RB, not a
toggle.

**Change.** Grab is now a held button: hold the GrabWorld button (RB) to grab the nearest prop in reach and keep
carrying it; release it to drop. In placement mode, releasing it confirms the ghost pose. B still toggles placement
on a rising edge. The buttons live in `CarryButtons` (defaults bit 28 grab, bit 20 placement) on `PropCarry`, so a
host setting or mod can rebind them. `Tick::from_controller` builds the tick from the derived controller words.
The physics (push, drag, depenetration) is unchanged.

**Files.** `crates/skate-game/src/physics/prop_carry.rs`, `crates/skate-game/src/physics/frame.rs`, tests in
`crates/skate-game/src/physics/prop_dynamics.rs`.

**Verification.** New `sprinting_past_a_prop_does_not_grab_it`: A pressed then held for 90 ticks while the carrier
runs past a prop 0.8 m to the side at 5 m/s. No grab, and the prop moves less than 5 cm. It fails with the old A
binding ("sprint (A) produced a grab on tick 0") and passes now. New `grab_world_button_holds_and_release_drops`
covers hold, carry, release and the B edge. The existing carry and placement tests now use the held grab and pass:
16 prop tests pass. `cargo test --locked -p skate-game`: 454 pass, 1 known upstream failure (`pipelines_accept_…`).
`cargo build --locked -p skate-game` (dev) succeeds.

**Open.** Retail's grab qualification (scene query, angle limits 436/452, margin 444) is not ported. The host still
uses #15's 2 m any-direction nearest-prop rule. The bench FPS drop with the board stuck inside it is a separate
issue (prop depenetration has no cap) and is not addressed here. There is no Lua API for `CarryButtons` yet. In-game
check by the user: sprint past the Aletown bins (they should stay put), then hold RB next to one (it should drag).

## Moving a held prop: every stick direction goes forward, skater inside the prop, grab flicker, 2026-10-05

**Problem.** User sessions 13:20 and 14:46 (fix13 in the build): "no matter what direction on my stick I pushed I
went one direction", "No matter what direction I push or pull an object it just goes forward. that includes to the
sides", "Grabbing objects warps you inside of it instead of grabbing the edge of it". The 14:46 log also shows the
physical state flipping BipedGround <-> OffBoardPushing every tick for 6 ticks at 20:47:32Z.

**Root cause (direction).** fix13's Move Object path (`biped_ground::update`, state 502 with a held prop) drives the
pair through the walking controller's velocity override. The OB_ObjectMv inputs and the controller velocity
(Biped480) were right for every direction, but the controller's approach step (82D7F458..FDD0,
`ground_motion/approach.rs`) [code] steps toward the ground contact target projected onto the FACING line
(side axis = cross(up, forward) removed), with a budget of |velocity| * dt. The target sits ahead of the facing, so
a pull or side step came out as a forward step at the same speed. Walking never shows this because the walker
turns to face its velocity.

**Retail.** [code] State 502 is its own class: ctor 82D43B90 (1216 bytes, vtable 0x82327364; an earlier note had 0x82317364, a typo), stored at
Player+1776 by the player constructor (82DB2DFC); BipedGround's object (ctor 82D305D8) sits at Player+1764. The
walking controller job 82D4E2F8 (the only caller of the ground controller 82D7C818) is submitted only from
BipedGround's update 82D30D30, so retail Move Object does not take the facing-line approach. Its own movement
(probably in 82D44A10 / 82D45D30, which call the shared foot and ground-sync helpers 82D773F8, 82D77558, 82D2E250)
is not decoded.

**Root cause (inside the prop).** `prop_carry::follow` pulled the prop's CENTRE in to 0.9 m (`DRAG_HOLD`). A bench
(box half extents 2.64 x 0.22 x 0.38 m) grabbed by its end then sits around the skater.

**Root cause (flicker).** The auto-drop test measured the prop's centre against `MAX_HOLD_DISTANCE` (3.5 m) while
the grab reach is measured to the box surface (2 m). A long prop grabbed by its end has its centre beyond 3.5 m, so
it was dropped the tick after every grab and re-grabbed the tick after that while RB stayed held.

**Change.**
- `biped_ground.rs`: for the Move Object job only (502, held prop, stick asking for motion), clear the job's
  target-contact bit (flags bit 2) so the approach steps by the requested velocity; the support contact (bit 1)
  still keeps the feet on the ground. Labelled NOT RETAIL YET in the code: it loses the 0.3 m step-up of the
  target contact while dragging.
- `prop_carry.rs`: the hold distance is at least the box's extent toward the skater plus `grip_reach` (new carry
  tuning, 0.35 m, NOT RETAIL YET: engine value; retail's grab offset probably comes from the DMO characteristics and
  the MovingObjectNew / MVOBJ hand positions, UpdateObjectGrabbing 0x821BCDB4, IsGrabbingDMO 0x820B757C). The
  auto-drop limit is `MAX_HOLD_DISTANCE` plus that extent, so it is measured from the near face like the grab.
- Mods: `sdk.world.set_tuning('carry', {grip_reach = ...})` (finite, >= 0; reset on mod disable like the other carry
  fields), shown by `sdk.engine.inspect(..., 'world_tuning:carry')`; `sdk/skate.lua`, `api.lua` updated.

**Files.** `crates/skate-game/src/physics/biped_ground.rs`, `crates/skate-game/src/physics/prop_carry.rs`,
`crates/skate-game/src/physics/carry_direction_tests.rs` (new), `crates/skate-game/src/physics.rs` (test module),
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`,
`crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.** New asset-backed `move_object_follows_the_left_stick_in_the_skater_frame` (DownTown, stock graphs
and setup data, pad input through the whole frame): step off, put the nearest map prop (a 5.3 m bench-sized box)
end-on with its near face 1.5 m ahead, hold RB, then push the left stick in 8 directions for 45 ticks each. Before:
every direction moved the pair forward (stick right: local [0.01, 0.59] m; stick back: [0.00, +0.72] m). After:
forward [0, 1.05], right [0.60, 0], back [0, -0.75], left [-0.60, 0], diagonals cos >= 0.97 (speeds differ per
axis: push 1.4, side 0.8, pull 1.0 m/s); the prop follows; gap skater to near face 0.35 m throughout (before: centre
at 0.9 m); exactly one state change while grabbing (11 flips with the old centre limit). Prop height drift on the
flat spawn plaza is under 2 cm. Run: `SKATE3_ASSET_ROOT=<assets> SKATE3_MAP=<maps/DownTown.skate> cargo test
--locked --release -p skate-game --bin skate3rust carry_direction -- --ignored`. `cargo test -p skate-game --bin
skate3rust`: 489 pass, known `pipelines_accept_...` failure, and `living_world_npc_trick_jump_has_no_pose_pop`
failed once in the full run but passes alone (NPC work in progress, not this change). skate-core
`input::offboard` 11 pass; skate-mods 96 + 3 pass, known Skyline failure.

**Open.** Decode the 502 class's movement (82D44A10 / 82D45D30) and replace the bypass. The bench sinking
through the ground by the wide plaza stairs (14:46 video at about 10 s) is not reproduced on flat ground; the held
prop is moved by `drag_to` (horizontal velocity forced every tick, vertical left to the solver) while its own
triangles are parked, so a step riser or edge under a long box is the likely place; a repro on those stairs and a
look at the held-body depenetration in `prop_dynamics.rs` come next.

## Board stuck inside a prop, 2026-10-05

**Problem.** User's first play: "Interacting with this bench near aletown spawn broke the framerate, also its hitbox
seems a bit off ... The framerate returned when I called back my board, so it may have been stuck inside the bench".

**Root cause (confidence: high for the cost, medium for how the board got in).** The bug is in #15's original code
(`physics/prop_dynamics.rs`, `push_from_skater`). Each prop's contact box is the AABB of its template's render
triangles, while the board itself collides with the exact render triangles. Under a bench seat the mesh is open
but the box is solid, so the board can sit inside the box. #15 then pushes the prop with a 0.5 m/s floor every tick
any skater volume penetrates it, even when nothing is closing, and board pushes had no speed cap. For a bench-sized
prop (about 126 kg at the default density) that nudge is almost exactly what ground friction removes again, so the
bench creeps but never separates and never cools down to sleep. Every awake tick rebakes its triangles, and
`BoardWorld::replace_triangles` rebuilds the query index of the whole prop layer. Calling the board back removes
the overlapping volume, which ends it.

**Evidence.**
- `legacy_push_keeps_bench_awake_with_parked_deck` (deck capsule parked inside a 2.0 x 0.9 x 0.7 m box, #15's
  tuning): over the last 300 of 600 ticks the bench is awake 300/300, pushed 300 times, rebaked 300 times, travel
  0.135 m.
- `downtown_prop_rebake_cost` (ignored, private assets): DownTown has 786 props and 151885 prop-layer triangles; one
  rebake of the Aletown bench template (`ac_DMO_DT_benchAggregate_1001...`, 224 triangles, box half extents
  1.61 x 0.54 x 0.43 m) costs about 0.5 ms in the test build. That is paid every physics tick while the bench is
  awake, plus the per-tick pair queries.
- No retail address for DMO contact response or depenetration was found in this run; the values below are engine
  choices (#15's constants plus caps), not retail.

**Change.**
- All prop contact and push values are now one `PropTuning` (`physics/prop_dynamics.rs`), defaults = #15's
  constants plus: `max_depenetration_per_tick` 0.05 m (the positional correction of one body per tick is clamped),
  `board_push_speed` 6.0 m/s (board pushes are capped along the push direction like body bumps), and
  `stuck_release_ticks` 30 (after 30 ticks of a skater volume sitting inside a prop without closing on it, the
  overlap nudge stops and slow drift no longer pushes; a real hit faster than `penetration_push_speed` still does).
- Per prop type overrides by MOBJ template name (`PropTuningTable.by_template`), including an optional
  template-space `collision_box` that replaces the render-AABB box (the rendered pose stays put; mass unchanged).
- Bevy resource `PropTuningSettings` holds the table; `apply_prop_tuning` pushes it into the live props before each
  physics tick and after a map load. Setting it to `default()` (mod disable) restores the shipped values; only
  prop types whose tuning changed are woken.
- A prop whose pose did not change since its last rebake skips the rebake (bit-identical pose, so identical
  triangles; behaviour unchanged).

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs`, `crates/skate-game/src/physics.rs`.

**Verification.** `cargo test --locked -p skate-game prop_dynamics`: 18 pass (all earlier prop tests unchanged) plus
the ignored DownTown probe. New: `parked_deck_inside_bench_lets_it_sleep` (awake 0, rebakes 0, pushes 0 over the
last 300 ticks, travel 0.001 m), `released_bench_still_takes_a_real_hit`, `board_push_speed_is_capped`,
`depenetration_is_bounded_per_tick`, `tuning_overrides_per_template_and_resets`. Full skate-game: 461 pass, 2 fail:
the known `pipelines_accept_valid_group_outputs_when_fingerprint_changes` and
`graphics_menu::sections_expose_only_real_rows_and_all_maps` (graphics_menu.rs is being changed by the NPC draw
distance work, not touched here). Dev build OK. Not yet checked in game.

**Open.** The board itself still sits inside the open bench mesh until the player moves it; the frame cost is gone
because the bench sleeps. What retail uses for this DMO's collision (its own physics volume vs the render mesh) is
not recovered; a per-type `collision_box` can fit the bench once it is. Lua (API 2) bindings for `PropTuningSettings`
are a follow-up. `replace_triangles` still rebuilds the whole prop-layer index per moved prop (a refit would be
cheaper but needs proof that query order is unchanged).

## Dragged props sinking through the floor, 2026-10-07

**Problem.** User: "the shit with them falling through the ground is SO annoying. its a top priority fix". Earlier:
"Dragged props sink through the floor and pull back toward their start spot". Props moved with Move Object (RB:
bench, bin, rail, vending machine) fell through the floor and kept sinking; peds then ignored them (a moving prop is
never cut into the navmesh, see "Peds walking through props").

**Evidence [trace].** `logs/game-20261007-225751` (DownTown, `PED_OBSTACLE`): 7 of 8 moved props went from a bottom
of about 12.6 m (spawn) to 8-10 m within 1-3 s at 7-9 m/s (free fall) and stayed `moving`; one (206220507, moved
1.12 m) rested normally. The sinking starts while the prop is still held: bench 3417526289 was already 0.6 m low at
the release sample (centre 11.47 against a spawn of 12.07) and its height span had grown from 0.81 m to 0.92 m,
i.e. the box was tilted. Every obstacle footprint grows after release (bin 44597382: [0.28, 0.27] -> [0.39, 0.32]).

**Root cause [code].**
1. `PropDynamics::drag_to` (and `set_yaw_rate`) overwrite the held body's angular velocity every tick, but the
   step still integrated the contact corrections' angular part: the floor friction on a box pushed along the ground
   (a spin about its bottom edge) was added to the orientation every tick and never undone, so pitch and roll built
   up over the drag. The drag doc comment already said "Rotation stays frozen"; only the velocity was frozen.
2. A tilted box digs a corner into the floor; once its centre passes the floor plane, the separating axis points
   down and the retail triangle fixup (`fix_up_triangle`, 82AD3130) rejects the contact on a one-sided face
   (`ONE_SIDED`, projection < 0). From then on nothing holds the box and it falls forever (the 7-9 m/s in the log).
   District collision is one-sided (`portable_world` / the retail archive flags), so this is permanent.
3. Not the cause: the prop's own triangles. Props collide with the map's `collision_world` only; their own layer
   triangles are parked at `HELD_PARK` while held and are not part of the prop step's world.

**Retail [code, partial].** Move Object is state 502 (ctor 82D43B90). Its update 82D44A10 calls 82D444A0, 82D45D30,
82D463D8 and 82D46218; 82D45D30's direct accesses are the state's own fields (+8, +16, +1128, +1200) and its
helpers 82BE3220 / 82D2D2B0 / 82D448D8 call nothing further (math or state helpers); no rigid-body write was found. 82D463D8 blends a 4x4 transform toward a target through 82E0A570
(row lerp plus renormalise 82BD3150) on the state's +1164 timer; whether that transform is the held DMO's is not
confirmed. So how retail holds the dragged object's orientation is NOT decoded. Not retail yet, labelled in code.

**Change (first interim, REMOVED 2026-10-08).** A yaw-only rule for the held body's contact rotation; the user
rejected it (props must tip) and the Move Object port below replaced it. What stays from the interim:
Diagnostics:
- `HELD_PROP` (held prop every second, released prop every second for 3 s): id, phase, template, centre, local up
  axis Y, velocity, ground height under it, gap (box bottom minus ground), contact manifolds, asleep, tick.
- `PROP_BELOW_GROUND` (any awake prop whose centre is more than its half height below the floor under it, checked
  twice a second, once per prop per 10 s): centre, half height, ground, last rest height, velocity, up Y, held,
  contacts, tick. The ground probe starts above the prop's last rest height, so a sunk prop still finds the floor
  it fell through.
- A sunk body is logged, not recovered: retail's handling of a DMO under the world is not decoded and no heuristic
  teleport was added.

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs` (held constraint, `PropGroundProbe`, `ground_probe`,
diagnostics, tests).

**Verification.** Pending (see todo `ped-moved-prop-collision`): `dragged_props_rest_on_the_floor_after_release`
(bench, bin, vending and rail boxes from the log's half extents, pushed 5 s over a DownTown-like street of 1 m
one-sided tiles with a 0.15 m curb, released, 3 s settle: upright while held, never more than 8 cm into the floor,
resting on the street, no slide back toward the spawn), `ground_probe_flags_a_body_under_the_floor`, and the
data-gated `downtown_dragged_props_rest_on_the_floor` (the session's prop ids on the real DownTown collision).

**Open.** The "pull back toward the start spot" part of the first report is not reproduced; the next session's
`HELD_PROP` lines will show it if it remains. Retail Move Object object transform (82D463D8 and callers) to decode.
Placement mode (`carry_to_pose`) snaps the orientation and is unaffected.

**Update (2026-10-07, late).** User on the interim: "props should still be able to tip.. they do in retail", then
"we should try to solve how retail handles the whole system as the source of truth, because its not a problem in
the original game". The yaw-only held rule is rejected and will be removed. Retail [code]: every held tick
82D45318 sends the held DMO a bounded command through the player DMO interface (vtable slot 9): a horizontal
linear term and a yaw term only (vertical gain 0, no pitch or roll), from two controllers on the velocity error
against the DMO velocity (gains [20, 0, 40, 0.1], 82D4E118; linear clamp 20, rate 4 per tick, yaw clamp 6.0),
minus the push into a smoothed blocking normal. So tipping, gravity and floor contacts stay free physics and the
push never beats the floor solver. The prop leads; the skater follows its grab edge (82D45D30, grab record at
state+720); stick input is object-relative. Retail values [data] attribute class 3EDA5B140604613D (push 3.0, pull
2.0, side 2.5, mass and yaw-inertia curves), per 60 Hz tick. Port plan and open points: research spec (local) and
todo. The interim test `dragged_props_rest_on_the_floor_after_release` failed ("the push dropped the prop": the
synthetic push lost the grab); the port replaces it.

### Move Object port (2026-10-08)

**Change.** Retail Move Object as the source of truth (research spec `move-object-retail.md`, local):
1. `skate_core::player::offboard::move_object` (new, pure, `#![forbid(unsafe_code)]` crate): `command()` is a port
   of 82D45318 steps 1 to 10: lever arm along the grab edge, rotation demand (E4FF0185DA44CDBD), yaw-rate target
   (BFB3BEF0BB2661C0 x yaw gain EABFCC79873A2859), push / pull / side targets x mass speed scale (57D37D696363167E),
   lever coupling -0.25 (0x8208ED00), heading latch (0.1 rad, 557FA142008FD7CE), smoothed blocking normal (0.95 /
   0.5), the PhysicsControllerData update [code, 82D4E118, re-read for this port]: `filtered = 0.9 filtered + 0.1
   e`, `out += 20 e + 0 filtered + 40 (e - previous)`, the accumulator is not clamped; the clamp (20, 82BD3D90) and
   the slew (4 per tick, 0x82257308) act on the sent copy (+672), as in 82D45318. Yaw: `out += ...` on `w - drift x
   60`, clamped to 6. No vertical, pitch or roll term. `MoveObjectController` is the whole per-slot state, plain
   `Copy` data with a flat `to_array` / `from_array` form (multiplayer-ready, deterministic, fixed tick).
2. `PropDynamics::apply_move_command` replaces `drag_to` / `set_yaw_rate`: the command is ADDED to the held body's
   horizontal velocity and yaw rate before contacts. NOT RETAIL YET: the DMO side of interface slot 9 is not
   decoded; the command is applied as an acceleration (m/s^2, rad/s^2) for one tick. The yaw-only rule is gone; the
   held body is a normal rigid body (gravity, contacts, tipping).
3. The prop leads, the skater follows: `PropCarry` publishes the grab frame (grip point on the held face, edge
   normal, skater spot `grip_reach` back); `biped_ground` pulls the skater onto it through the walking job's
   velocity override (capped at the linear clamp) and turns it to face the edge. NOT RETAIL YET: 82BDF268 and the
   frame blend rate are not decoded. The old follow, `MAX_HOLD_DISTANCE`, `DRAG_HOLD` and `MAX_DRAG_SPEED` are gone.
   The fix20 target-contact bit drop stays, now as part of the follow move (removing it would turn a follow along
   the edge into a forward step; retail never runs that job in 502).
4. Interim grab record (NOT RETAIL YET: DMO grab-spline source undecoded): the vertical box face toward the skater
   at grab time, grip along its edge clamped 0.25 m inside the ends, kept in the prop's frame. Let go (NOT RETAIL
   YET, retail: the record stops qualifying, 82E08DB8 / 82E08EE8): the skater falls more than `let_go_distance`
   (1.0 m) behind the closest it got to its grab spot, or leaves the on-foot states, or RB.
5. Tuning as data: `load_move_object_tuning` reads class `Hash_3EDA5B140604613D` from the stock collection into
   `PhysicsSettings::move_object` (built-in stock values only if the collection lacks it, with a warning); the
   live carry gets it as its base on map load. Mod entry `sdk.world.set_tuning('carry', {...})` gains
   `linear_clamp`, `yaw_clamp`, `relatch`, `slew_per_tick`, `linear_controller`, `yaw_controller`, the four curves
   and `let_go_distance`; `push_speed` / `pull_speed` / `side_speed` now default to retail 3.0 / 2.0 / 2.5;
   `turn_rate` now means a constant yaw gain replacing the inertia curve. Overrides sit on the setup-data base and
   are cleared on mod disable; `world_tuning:carry` reads the values in effect.
6. HELD_PROP gains `stick=[x, z, rot]` (OB_ObjectMv), `command`, `yaw_cmd`, `lever`, `rot`, `blocked`, `drift`.
7. Held body never rest-snaps or sleeps while held (engine rule, retail keeps the held DMO through interface slot
   10; its sleep rule is not decoded): the prop rest snap zeroes any velocity under sqrt(0.5) = 0.7 m/s while
   touching, which ate every tick of the command and pinned the prop.
8. Static contact impulse shared over ALL simultaneous points (prop step, `contact_corrections`): each touching
   floor triangle resolved the full closing impulse from the same velocity, so a box edge on ~10 tiles got ~10x
   the impulse. Trace: a tipping bench was launched at 15 m/s upward and 17 rad/s, then fell through the floor
   ([trace] test street drag, 2026-10-08). Shared, the bench no longer leaves the floor (worst gap -0.03 m).

**Verification (2026-10-08).** `cargo test -p skate-core move_object`: 8 pass (stock curves, centre push, off-centre
turn sense, controller step response hand-computed, blocking normal, heading latch, state round trip and
determinism, mod value fallback). `cargo test -p skate-game --bin skate3rust -- prop_ carry world_tuning`: all pass
except `dragged_props_rest_on_the_floor_after_release`: bench, vending and rail stay on the floor (worst gap -0.03 /
-0.08 / -0.05 m) and rest; the bin still falls through (open 2) and the vending machine ends 0.4 m nearer its spawn
because it fell over backwards; all four tipped over under the saturated push (open 1); `pushing_a_tall_prop_into_the_curb_can_tip_it` passes (tipping kept). Four sim tests are
`#[ignore]` with the reason "blocked on the DMO interface decode": straight push speed, left stick in the edge frame,
right stick turn, push distance. Full `skate-game` run: the other failures (7 ped tests: `PedObstacleTrace` resource
missing in those test apps; `setup::pipelines_accept_valid_group_outputs_when_fingerprint_changes`) are in code this
change does not touch.

**Open.**
1. RESOLVED (see "Slot 9 applied" below: anti-windup write-back, commanded block). Was: slot 9 semantics decide everything left: read as an acceleration at the centre of mass, the integrating
   controller (no anti-windup in 82D4E118 / 82D45318) saturates at 20 m/s^2 within two ticks for ANY stick, so it
   overshoots the target speed (4.1 m/s for a 3 m/s target), tips a 1 m cube (tips above g x half width / half
   height) and makes the yaw latch oscillate (0.66 rad of drift from the left stick alone). Retail props do not
   behave like that, so this reading is probably wrong. Next: the first-pass hook at the `bctrl` in 82D45318 (spec
   section 6.1) to name the DMO interface and the command units. No values were tuned around it.
2. Contact gap: FIXED for the lying box (see "Contact gap" below). The bin still falls during the held drag because
   the current command application tumbles it corner-first (open 1). Resolved by "Slot 9 applied": the bin stays
   on the floor (worst gap -0.039 m).
3. A tipped prop is still "held" (interim grab record ignores tilt); retail's record would stop qualifying.
4. Skater contact normal (Player+16304) is not wired into the blocking normal yet (zero).
5. The rest-snap exemption (7) and impulse sharing (8) are engine rules, not decoded retail.

**Slot 9 applied (2026-10-08, later).** Problem: with the command applied as a plain velocity add and the
controller accumulating without bound, a full push overshot (4.1 m/s for a 3 m/s target) and tipped every prop;
the user: dragged props sink ("its not a problem in the original game") and "props should still be able to tip..
they do in retail" (from obstacles, not from every push).

Root causes [code, TU3 recomp, read for this change]:
1. Anti-windup missed in the port. 82D45318 stores the clamped and slewed sent command (+672) back over the linear
   controller output (+1008 +16 = +1024, `stvx128 v0,r31,r4` with r4 = 1024 right after the slew), and stores the
   clamped yaw command back over the yaw controller output (+1088 +16 = +1104). The earlier port read "the running
   output is never clamped"; with that reading the retail controller math alone predicts a 9.7 m/s peak on a free
   3 m/s push. The overshoot was the port, not slot 9 and not the one-tick-old record velocity.
2. The commanded parameter block was not applied (spec 7.4 item 4): with the authored floor friction (0.5 to 0.6)
   every push tripped the prop over its base.
3. Two signs dropped in the yaw term: the code has `lever = dot(-edge (+320, sign-flipped by vxor), centre (+880)
   - grip (+256))` and `w = -(curve BFB3BEF0BB2661C0(|lever|) x rot x gain +1160)` (`fneg f24,f13`); the port had
   neither minus, which cancels for the lever-driven turn but flips OB_ObjectMvRot. The coupling
   (`fnmsubs`: fwd = fwd - lever x w x (-0.25)) was already equivalent and is now written as in the code.

Change:
1. `skate_core::player::offboard::move_object::command`: anti-windup write-back for both controllers; lever and
   yaw target with the code's signs. Edge direction (+320) taken as `side_of(forward)`: retail builds it from the
   hand points (82D45D30) and its sense is not traced; it is the only choice for which an off-centre push turns the
   object the way its push torque does (r x F about +Y). NOT RETAIL YET: the edge sense. Our integrator is
   right-handed about +Y like the slot 9 angular sink (`(0, out, 0)`), checked by
   `positive_yaw_rate_turns_local_z_toward_plus_x`.
2. `PropDynamics::apply_move_command(id, linear, yaw, grip, dt)`, per 60 Hz step, before the contact solve
   (spec 7.5): unknown or massless body skipped (props have no lock state yet, retail gate DMO+4464 bit 0x08); wakes
   the body and clears its sleep counter on EVERY command, zero or not (82ADF7B8); `v += L dt` at the centre of mass,
   no mass factor, no torque, vertical dropped (82C4C370 passes only &linear); the yaw command replaces the angular
   accumulator (`torque_acceleration` zeroed, `w += (0, Y, 0) dt`, no inertia factor; contacts still change pitch
   and roll). Gravity stays in our integrator for every prop (retail resets the linear accumulator to the island's
   gravity, the same quantity). Our prop step runs once per tick, so the command acts for one step. The push never
   carried a lever-arm torque in the port (the lever only feeds the yaw command, as in 82D45318): nothing removed.
3. Commanded block (82C53EF8): a commanded body switches to `{0.03, 0.02}` on its next step and back to its free
   material on the step after the commands stop (bits 0x02 / 0x01 of DMO+4465 as `commanded` /
   `commanded_block`). Superseded 2026-10-08 (doc 27, "Contact material blocks"): the reader is found; the block
   is the body's own contact material {static friction 0.03, dynamic friction 0.02, restitution DMO data +272},
   combined with the other side by 82763078 (max / max / min). The interim "replace the combined friction"
   mapping described here is removed. Its reasoning ("the combine takes the greater friction, so 0.03 alone would
   change nothing") held only against the 0.8 / 0.6 test floor; the game gives prop contacts the retail ground
   material {0, 0, 1}, under which the body's own block decides. Damping and max speeds are untouched while held.
4. Rest snap: kept off for a commanded (or held) body. Sleep: retail clears the sleep counter on every command, so
   a commanded body cannot sleep (retail). The snap itself is our engine rule (it zeroes velocities under 0.7 m/s
   while touching) and would eat the first ticks of the command (slew 4 m/s^2 per tick), so the exemption stays,
   now tied to the command; held still covers placement (`carry_to`).
5. Interim grab record fix: the face toward the skater is now the face whose plane the skater is furthest outside
   of (projection minus half extent), not the largest raw projection, which picked the end face of a long bench for
   a skater behind its long side. Still NOT RETAIL YET (DMO grab splines undecoded).
6. Moddability: `sdk.world.set_tuning('carry', {...})` gains `commanded_material` ([0.03, 0.02]), `apply_at_com`,
   `yaw_replaces_torque`, `ignore_vertical`, `wake_on_command` (all true = retail) and
   `by_template[<MOBJ template>] = {material_held, material_free}` (friction pairs `[static, dynamic]`, validated
   finite and non-negative, at most 256 templates; 2026-10-08 also `upright_cos`, `material_free_upright`,
   `upright_pair`, `restitution`, see doc 27). They live in `CarrySettings::move_rules` (`MoveCommandRules`), are pushed
   into `PropDynamics` every tick (a map load keeps them), read back by `world_tuning:carry`, and go back to retail
   on mod disable. Multiplayer: the command is plain data (id, L, Y) applied once per fixed tick in the prop step;
   the per-body flags are two booleans; no wall clock.

Verification (2026-10-08): `cargo test -p skate-core`: lib 762 pass / 2 fail (the two known HEAD failures),
integration 155 pass; `move_object` 9 pass (new: yaw write-back; the step response is hand-computed with the
write-back: 4, 8, ..., 20, then 16 at the target speed). `cargo test -p skate-mods`: lib 102 pass / 2 ignored;
`skyline_physics` fails (asset missing, as before). `cargo test -p skate-game --bin skate3rust`: 544 pass /
2 fail / 184 ignored (before: 535 / 2 / 187); new passing tests: `dragged_prop_follows_a_straight_push` (2.950 m/s at 3 s, retail math
2.951, target 2.951; peak 3.259 vs retail math 3.251), `grabbed_prop_moves_with_the_stick` (2.686 m in 1 s, retail
math 2.685 m), `move_object_left_stick_moves_the_prop_in_the_edge_frame`,
`right_stick_turns_the_held_prop_and_the_skater_follows` (sign from the code), `off_centre_push_turns_a_long_prop`
(turns with its torque), `straight_push_on_flat_ground_does_not_tip_a_cube_or_the_bin` (up_y min 0.9996 / 0.9970),
`commanded_block_switches_with_the_command_and_zero_commands_wake`,
`move_command_is_an_acceleration_at_the_centre_of_mass`, `positive_yaw_rate_turns_local_z_toward_plus_x`,
`carry_move_command_rules_set_and_reset`; `pushing_a_tall_prop_into_the_curb_can_tip_it` still passes (up_y min
-0.08: tipping from obstacles stays). The "retail math" reference is `predicted_centre_push`: the ported
controller alone on a free point mass with our timing (velocity read before the step, command applied in it).
`dragged_props_rest_on_the_floor_after_release`: bench, bin and rail pass every check (worst gap -0.033 / -0.039 /
-0.052 m, the bin no longer falls through); the vending machine fails by 0.07 mm (worst gap -0.0801 m, limit
-0.08, not loosened): the overlap happens after release, not while held. It is let go at about 3 m/s, the free
block (authored friction) returns on the next step, it trips (friction 0.5 against half depth / half height 0.47)
and lands on its back at 1.6 m/s with 7 manifolds; the box overlaps the floor up to 8 cm for a few ticks and then
rests at gap 0, asleep. Residual prop solver gap (impulse shared over all simultaneous points, 40 % positional
correction, no speculative contacts), not the Move Object command and not the 0.03 interim.

NOT RETAIL YET (this change): the block mapping (friction only), the free block source, the edge direction sense,
the rest-snap exemption (engine rule), the grab-face choice. Still from the port: skater follow move (82BDF268),
interim grab record and let-go distance, skater contact normal not wired, impulse sharing.

Open:
1. RESOLVED (see "Yaw-rate feedback" below). Was: heading latch while turning: the port measures the drift only while |w| < 0.1 and lets the latch follow the
   object otherwise, so the yaw controller has no rate feedback while turning and pins at the 6 rad/s^2 clamp (a
   held right stick spins a 1 m cube to 6.4 rad/s in 1.5 s; the off-centre bench turns 10 rad in 2 s). In the code
   both the turning branch (0x82D45714) and the small-drift branch (0x82D4570C) store the latch (+368 / +400) every
   tick, and a second wrapped angle is built from the stored drift with 8296EC98 (+384) before the yaw error; that
   is probably the rate feedback. Not decoded; `held_right_stick_turn_rate_stays_bounded` is ignored until it is.
2. The block's reader (spec 7.6 item 1) and the DMO free pairs (7.6 item 2).
3. The prop landing overlap above (solver).

**Yaw-rate feedback (2026-10-08, later).** Problem: a held right stick spun a 1 m cube up to 6.4 rad/s in 1.5 s
and an off-centre push turned the bench 10 rad in 2 s: the yaw controller had no feedback while turning and sat at
the 6 rad/s^2 clamp. Retail props turn at a controlled rate.

Root cause [code, TU3 recomp, 82D45318, static reading]: the spec's "error = w - drift x 60" was a misreading. The
drift against the heading latch (+368) only decides the re-latch; the yaw error uses a different angle:
1. Latch (0x82D455E0..0x82D4573C): turning (|w| >= 0.1, 0x820641A8) stores the latch (+368 / +400) every tick
   (0x82D45714). Not turning: drift = wrapped angle between the facing and +368 (8296EBB0, wrap with 1 / 2 pi
   0x82139A60 and 2 pi 0x82139A50); above 557FA142008FD7CE (0.1 rad) [data] the latch is stored, below it the
   store is skipped (0x82D4570C sets only the "turning" flag r23 = 0, which feeds the +1172 idle timer). So the
   small-drift branch does NOT store; the port's latch was already right. The drift is not read again (v127 is
   reused for the height error before the yaw part).
2. Rate (call returning at 0x82D45BC8): `8296EC98(out, +384, facing now, axis (0, 1, 0) at 0x82139A20)`, then +384 =
   facing now (0x82D45C0C), every tick, in every branch. 8296EC98 [code]: both vectors normalised (refined rsqrt); if either
   |v|^2 <= 1e-4 (0x8209BE90) the result is 0 (0x82165A10); else a = acos(clamp(dot, -1, 1)) (82453298) and, when
   cross(+384, now) . axis < 0, 2 pi - a (2 pi at 0x821647F0). The result is wrapped to [-pi, pi) as above and
   multiplied by 60 (0x822F860C, loaded at 0x82D45C08): the measured yaw rate (|rate| stored at +1192).
3. Yaw controller (0x82D45C3C..0x82D45CD0): error = w - rate (`fsubs f7,f24,f10` at 0x82D45C3C), then the same
   PhysicsControllerData update as before (gains +1088..+1100 = B46764285AD1DC5F [data] 20 / 0 / 40 / 0.1, output
   +1104, previous +1108, filtered +1112, derivative +1116), clamp +-6 (AD327350D151B1E3 [data]), clamped value
   written back to +1104 and sent through slot 9.
   With the output accumulating, the loop is PI on the yaw rate: on a free yaw body it spins up at 0.1 rad/s per
   tick (clamp 6 / 60), peaks 0.6 % above |w| and settles at |w| with zero steady error.

Change:
1. `move_object::command`: yaw error = w - 60 x wrap(heading - previous heading); the previous facing (+384) is new
   controller state (`facing_yaw`, `facing_valid`; first held tick measures 0 like retail's zero vector), stored
   every tick. The latch drift no longer enters the yaw error. `MoveObjectCommand::yaw_rate` added; HELD_PROP logs
   `yaw_rate=`. Flat controller form grows to 33 floats (31 / 32 = facing).
2. The factor 60 is the tuning field `yaw_rate_feedback` (was the unused-elsewhere `tick_rate`), mod knob
   `sdk.world.set_tuning('carry', {yaw_rate_feedback = ...})` (validated finite, >= 0; 0 turns the feedback off;
   read back by `world_tuning:carry`; back to 60 on mod disable).
3. The port measures the heading change about +Y (`HeldBody::heading`, atan2 of local +Z); retail measures the
   signed 3D angle between successive facing vectors with the sign from +Y. Identical for an upright object;
   differs only while the prop is tipped far over (NOT RETAIL YET in that case, minor).

Edge direction (+320): not settled by this code. 82D45318 only reads +320 for the lever (step 1); its sense comes
from 82D45D30 (hand points) and stays `side_of(forward)`, NOT RETAIL YET.

Verification (2026-10-08): `cargo test -p skate-core --lib`: 764 pass / 2 fail (the two known HEAD failures);
`move_object` 11 pass (new: `yaw_error_is_target_minus_measured_rate` hand-computed: tick 1 rate 0, -120 -> -6;
tick 2 at the target rate: 0 + 40 x 2 = 74 -> +6; wrap across +-pi; `yaw_rate_feedback_settles_at_the_target_rate`:
0.1 rad/s per tick for 10 ticks, settles at -2 within 1e-3, peak < 2.02). `cargo test -p skate-game --bin
skate3rust`: 546 pass / 1 fail (`setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`,
unrelated) / 183 ignored; `held_right_stick_turn_rate_stays_bounded` un-ignored and passing: 1.961 rad/s after 3 s,
peak 1.969, retail target |w| = 1.966 (expectation: within 5 % of |w| and peak < 1.05 |w|, from the retail math,
not fitted); `off_centre_push_turns_a_long_prop` turns -1.29 rad in 2 s (was about -10). `cargo test -p skate-mods
--lib` 102 pass / 2 ignored; `world_tuning` 9 pass (`carry_move_object_speeds_set_and_reset` covers the new knob).

To playtest: right stick while holding a prop (steady turn, no runaway spin), off-centre push on the bench (turns
and stops turning when the push stops), let go while turning.

Open: the edge sense (+320, 82D45D30); whether a tipped prop's facing vector (+176 source) is the box's local axis
or the grab frame (the port uses the box's local +Z heading).

**Contact gap (2026-10-08).** Problem: a box lying on its side, rolled 1 to 8 degrees, 2 to 5 cm into the tiled
one-sided street floor got no floor manifold at all, so the dragged bin fell through.

Root cause [instrumented test, every floor triangle under the box traced]: the narrow phase did not miss the
geometry. For every tile the SAT found an axis and the prism produced points; triangle fixup (82AD3130) then rejected
all of them. The per-triangle SAT (82ACF950) prefers the tilted box face (or an edge cross) by a fraction of a
millimetre over the floor normal, e.g. roll +3 deg, depth 5 cm: floor normal overlap 0.050 m, box face axis
0.0486 m. That normal is 1 to 8 degrees off the floor, so fixup classifies it as an edge or vertex region of a
welded flat edge (street flags 0xf10: one-sided, edge cosines, no convex bits, cosine 1, vertices disabled). The
props called the GP volume-pair query 82AD43A8 (`primitive_pair_contacts`), which hard-codes fixup's object flag to
false; on that path a flat non-convex edge with cosine 1 is above the bend threshold (0.999) and is dropped, and a
disabled vertex is dropped. Every tile dropped its contact.

Change: prop volumes against static world triangles now use the physics/world query 8277B720 (dispatch 8277BC58,
`primitive_triangle_world_contacts`), the retail routine for a moving volume against world triangles, with the query
context object byte (+61, read at 8277BC58 and forwarded to fixup as r9) set for props. On the object path fixup
accepts a flat edge while projection + convexity_epsilon >= cosine, i.e. a normal within acos(1 - 0.01) = 8.1 deg of
the face, which is exactly the window that failed, and does not reject disabled vertices. The limit is the body's
own padding (the gap the prop resolver accepts) with no velocity prediction (the prop solver has no speculative
rows; `maximum_separating_distance` 0). No new tolerance, no change in skate-core: the narrow-phase code is the
existing port. `PropDynamics::world_query_for`, `contact_corrections` in `crates/skate-game/src/physics/prop_dynamics.rs`.

Retail evidence level: the world query and the +61 byte are code facts; that retail props run with +61 set is
inferred from the flag's role ("is_object") and not traced (writers seen at 82715960 / 8271CCE8 set 1, 82722098
clears it; which query owners they serve is open). A body-level welding fallback (face-normal contact when fixup
drops a flat feature and no coplanar triangle publishes) was tried and removed: it is not retail and the object path
alone fixes the repro.

Verification: `lying_tilted_box_keeps_floor_contacts` un-ignored and passing (all 108 poses get floor contacts,
lowest floor normal up 0.990; it also asserts the old pair query still drops 11 poses so the repro stays honest).
Bench, vending and rail runs in `dragged_props_rest_on_the_floor_after_release` are bit-identical to before (they
never hit the gap); the bin still falls: it tumbles corner-first under the current command (compound tilt past
8.1 deg into flat vertex regions, where retail fixup also drops the contact), which the slot 9 recipe replaces.
Full runs: `skate-game --bin skate3rust` before 527 pass / 9 fail / 188 ignored, after 535 / 2 / 187 (the repro
un-ignored; the 7 ped tests fixed by inserting `PedObstacleTrace` in the test app). `skate-core` unchanged: lib
761 / 2, integration 155 / 0. Remaining failures: `dragged_props...` (bin, above; it passes at HEAD 0489702,
which predates the Move Object port), and `setup::pipelines_accept_valid_group_outputs_when_fingerprint_changes`
plus the two skate-core lib failures
(`broadphase_tests::predictive_contacts_and_retention_match_full_scan_for_every_primitive`,
`collision_feedback_tests::a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`), which
fail identically at HEAD 0489702 (temporary worktree, same target dir).

**Board drop, hold rule, skater follow (2026-10-08, later).** Problem: grabbing a prop kept the board in hand; the
grab was dropped by our 1.0 m distance rule instead of retail's; the skater was pulled 0.35 m behind the grip at up
to 20 m/s. Retail evidence [code, TU3 recomp, each address re-read for this change]:
1. Enter 82D442D0 (0x82D44304..0x82D44364): a `bdzf` switch on SkateboardController+448 (above 5 skips). 0, 1 and 5
   zero +444, call LetGoOfSkateboard 82D75440 and write +448 = 2 (the board becomes a free body where it is and
   keeps its velocity); 4 zeroes +444, calls 82D755E0 (hide) and writes 3; 2 and 3 are kept.
2. Hold rule in 82D44A10: CanGrabSpline 82E08EE8(record +720, reference bone 23 +272, grip distance +1128 splatted,
   box from 82D2E250 with GrabBoxSizeGrabbing (+0) and GrabBoxOffset (+32), angles +452 (GrabSplineAngleLimitGrabbing,
   80 deg, first angle test: reach vs -approach) and +436 (GrabSplineMaxAngleToHorizontalGrabbing, 50 deg, slope),
   both x the degree-to-radian constant at 0x8206D110). Failing it clears holding (0x20).
3. Skater follow, 82D44A10 0x82D45110..: target = edge point (+192) + 0.65 (0x820BB0EC) x latched row +368, y =
   Player+240 y + 0.72 (0x8220E144); 82BD41B0(target - +416, +624 x dt, 0.1 (0x820641A8)) returns v dt plus the
   rest clamped to 0.1 m; +416 += that step, then 82BDF268 moves the character. +624 (82D46610) = 0.85 v + 0.15 x
   (anchor change / dt), anchor = grip + 0.7 (96ECC98838ECCC11) x back, y dropped.
4. Frame blend 82D46218: rate = distance (+192 to +256) / (physics_state_offboard +448 x 60); only a ratio >= 1
   starts a blend (flag 0x08, 82D463D8); otherwise the frame is set at once. At retail data (1.0) that is a jump of
   60 m or more, so the facing snap we have is retail. No change.
5. Record+272 (from DMO type data +312, 82C4B960) doubles the target velocity in 82D45318 (2.0 at 0x82060C50).

Change:
1. `move_object::board_on_grab` + `ground_board::enter_move_object` (called from the 502 enter in
   `player_state/transition.rs`) let go / hide the board as above; `board_manager::Owner::hide`.
2. `move_object::still_holds` (82E08EE8 through `grab_scene::qualify_at`, the 82E08DB8 tests at a given arc
   distance) replaces the 1.0 m rule, also for the grab itself; `let_go_distance` stays as a mod-only engine rule,
   default 0 = off.
3. `move_object::SkaterFollow` (+416 / +608 / +624) replaces `grip_reach`; prop_carry publishes the root moved by
   this tick's follow step (the displacement 82BDF268 gets); biped_ground applies it without the 20 m/s cap. A first
   version targeted the follow point minus the live body (COM) offset: the COM swings with the animation and fed
   back until the skater wiped out (asset-backed test, DownTown).
4. Record+272: `MoveObjectInput::record_272`, per prop type `by_template[...].record_272`, scale
   `record_272_speed_scale` (2.0). Superseded 2026-10-08: the default is now the type's retail data +312 (doc 27,
   [Per-type DMO data](27-dynamic-props.md#per-type-dmo-data-2026-10-08)).
5. Interim grab record (NOT RETAIL YET): the held face's straight top edge (centre-height edges fell below the
   grabbing box's y range, 0.2 to 1.8 m above the root, and dropped a bench at once). Retail grab splines come
   from the DMO physics assembly definition +136 table, not in our assets: addresses recorded, no port.
6. Moddability: `set_tuning('carry')` gains `drop_board`, `follow_step`, `hold_angle_limit`,
   `hold_max_angle_to_horizontal`, `hold_box_extents`, `record_272_speed_scale`, per template `record_272`;
   `grip_reach` now sets the follow reach (0.65). Box, offset and angles load from physics_state_offboard
   `default`, anchor reach from the Move Object collection; all restored on mod disable.
7. Leaving 502: unchanged; the selector leaves when the grab byte (OffBoard304 -> Processed2476 bit 21) clears on
   every drop path; the asset-backed test checks BipedGround after the release.

Verification: `cargo test -p skate-core move_object` 15 / 15 (board switch, record+272, hold box and angles, follow
step and anchor velocity); `cargo test -p skate-game --bin skate3rust` 546 pass, 1 known
(`setup::pipelines_accept_valid_group_outputs_when_fingerprint_changes`); asset-backed
`carry_direction_tests` (DownTown, `--ignored`) 2 / 2 pass, including the new board state check (2 when carried,
3 when hidden); `skate-mods` passes except the known `skyline_physics` (asset missing).

Open: 82BDF268 (sweep, step-up, weight +1124) not decoded; the follow begins at the board-frame COM (our stand-in
for Skeleton+15872); per prop type record+272 and grab splines need the DMO data.

## Move Object step 1: props' authored grab splines in the export (2026-10-09)

**Retail [data] (`.local/research/npc/b48-prop-grab-splines-port-map.md`, `b54-dmo-grab-provider.md`; main checked the
GRABDATA header against parkassets and the per-template link below).** DMO templates carry authored grab splines (RW4
GRABDATA): 102 of the 136 worlddmo templates have one section. In a multi-template worlddmo arena the section sits
between the template's EB0001 model and the next template's model (the retail link `*(R+136)` is not decoded; the
positional rule matches parkassets' single-template copies byte for byte). In single-player worlds props answer the
grab query through the type-2 world-object provider (`82C4BE80`), not the DMO provider (`82589130`, used only in an
online session, b54).

**Change.** `tools/asset_pipeline/grab_data.py` `section_splines` (one section); `dynamic_props.template_meshes`
links each template's section by position and keeps `grab_splines`; the native-props export writes
`grab_splines[<template id>] = [{points, direction, bounds, flags}]` next to `types` (model frame, same fields as the
cars).

**Verification.** Python dynamic_props / grab_data tests; over all 119 parkassets DMO RX2s the per-template splines
equal the whole-file parse (main's run, 2026-10-09). The game does not read the field yet.

## Move Object step 2: props in the grab scene (opt-in, 2026-10-09)

**Retail [code] (`.local/research/npc/b54-dmo-grab-provider.md`; main checked `82C4BE80` emits type-2 records).** In
single-player worlds the world-object provider (`82C4BE80` box / `82C4BB08` radius, scene `+4084`, query mode bit 0x04)
answers props: disabled gate, a sphere around the object (radius + 15 m), per spline byte +68 > 1 and +70 != 0, a
segment hit, the assembly chain, then a type-2 record with the object's matrix and a vector (vtable +120), then
CanGrabSpline. Our `Provider::LivingWorld` query loop already ports these gates.

**Change.** skate-game: `skate_world::load_dmo_grab_splines` reads the export's `grab_splines`;
`PropDynamics::set_grab_splines` gives each body its template's splines; `PropDynamics::grab_objects` builds
`Provider::LivingWorld` objects (template origin and basis as the frame, points scaled like the body, linear velocity as
the vector, type-2 records, ids `PROP_GRAB_TAG | body id`); `GamePhysics::refresh_grab_props` replaces them in the grab
registry before the queries run each tick. `parse_grab_splines` is shared with the cars. Log `SKATE_PROP_GRAB`.

**Engine choices.** Opt-in with `SKATE_PROP_GRAB=1` until Move Object reads the authored record instead of its
stand-in edge (`prop_carry.rs` `grab_frame` / `choose_edge`; record choice `82D4D150` open): with the props live, the
biped grab query (mode 4) would bind authored records that the carry does not use yet. NOT RETAIL YET: the assembly is a
stand-in with the object id; the record vector is the linear velocity.

**Verification.** skate-game `a_prop_with_grab_splines_is_a_world_object_in_the_grab_scene` (168 prop / living_world /
offboard tests pass). Needs a setup refresh (or the next one) for the export field.


## Move Object step 3: carrying a prop by its authored grab record (opt-in, 2026-10-09)

**Problem.** Props carried by a stand-in edge (the box face toward the skater, top edge, grip clamped by an engine "hand
half spread") even when the prop has authored grab splines (step 1) that are already in the grab scene (step 2).

**Retail [code] (`.local/research/npc/b62-move-object-record.md`, main checked).**
- Record choice `82D4D150` is our `best_spline` (mode 0: from the skater position). Mode 1 (the held update's path B
  re-grab) skips records whose descriptor (+188 kind, +192 id) equals the held one.
- `82D444A0(state, full)`: `full` (enter `82D442D0`, path B) copies the record's reversed bit (+200 bit 0x20) into
  +1200 bit 0x04 and puts the grip (+1128) at the nearest arc distance of the skater reference (bone 23, +272) along the
  polyline (`82D2CFE8`), clamped to [h, length - h], h = min(`GrabSplineEndExclusion`, length / 2). So the clamp is the
  vault's end exclusion (physics_state_offboard `default` +444, 0.25), not a hand spread. Always: side test
  c = cross(rec+80 - ref, rec+64 - ref).y; c < 0 swaps the ends, negates the edge direction (+112) and toggles the
  reversed bit; if that bit then differs from +1200 bit 0x04 the grip is mirrored (length - grip), so it stays on the
  same point of the edge.
- Path A of the held update `82D44A10` refreshes the held record from the object's current pose every tick and runs
  `82D444A0` without `full` (grip kept). `82D45D30` evaluates the record at the grip (`82D2D2B0`).

**Change.**
- skate-core: `grab_scene::nearest_distance` / `at_distance` are public; `best_spline_excluding` (mode 1);
  `move_object::held_record` with `HeldGrip` (descriptor, grip, reversed), `begin_grip` (full), `continue_grip` (path
  A) and `record_frame`; `MoveObjectTuning::grab_end_exclusion` (0.25, loaded from `GrabSplineEndExclusion`).
- skate-game: `PropDynamics::grab_object(id)` (one prop's grab-scene object from its current pose);
  `PropCarry::frame_for`: a prop with authored splines binds the best record from the skater position at the grab
  (`begin_grip`), then each tick rebuilds its records, finds the bound descriptor and keeps the grip (`continue_grip`);
  the grab frame is the record at the grip (forward = minus the flattened approach vector, ends = record ends,
  grip distance = the grip). The hold rule (`still_holds`, 82E08EE8 at the grip) runs on that record. Props without
  splines keep the box stand-in.
- Mod: `sdk.world.set_tuning('carry', { grab_end_exclusion = ... })` (m, retail 0.25).

**Engine choices / NOT RETAIL YET.** Still opt-in with `SKATE_PROP_GRAB=1` (shared gate `prop_dynamics::prop_grab_enabled`
for the grab scene and the carry). The bound record comes from the prop's own splines (best from the skater position),
not from the player's published best record (Player+1888); path A refreshes by descriptor, not from Player+1888. Not
ported in this step (see step 3b): path B re-grab and its frame blend (`82D46218`), the hand points (grip +/- 0.5 x |bone3 - bone7|, 0x8209975C at 82D45DAC; b62 said 0.68, corrected by b64 and main), `82D43B20`.

**Multiplayer.** `HeldGrip` is the whole per-skater record state (plain `Copy` data); the record is rebuilt from the
host's prop pose every tick.

**Verification.** skate-core `move_object::held_record` tests (projected grip, end exclusion clamp incl. length / 2,
a flip keeps the world grip point, mode 1 skips the held descriptor); skate-game
`a_prop_with_grab_splines_is_carried_by_its_authored_record` (bound grip, facing from the approach vector, a 1 s push
keeps the binding). Not play-tested yet.

## Move Object step 3b: the rebind block, re-grab and hand points (opt-in, 2026-10-09)

**Retail [code] (`.local/research/npc/b64-move-object-regrab.md`; main checked the 0.5 hand factor at 82D45DAC and
that `82D43B20` is the getter of `96ECC98838ECCC11`, our `anchor_reach`).** Each tick of the held update `82D44A10`:
the collision timer +1176 counts 1/60 steps while Player+2484 bit 26 is set (else 0) and a push latch (+1200 bit 0x02)
sets once |Player+736|^2 > 0.04. The rebind block needs the hand flag, the timer at most 0.4 s and the latch clear;
path A (CanGrabSpline on the held record at the grip) refreshes the record; path B (`82D4D150` mode 1 without the held
descriptor, then `82E08DB8` with the end exclusion) re-grabs with a new grip, zeroes the anchor velocity and seeds the
anchor from the new record's nearest point; anything else drops the hold. The frame blend `82D46218` only runs for
jumps of 60 m or more (a snap otherwise). `82D45D30` places the hands at grip +/- 0.5 x |hand bone 3 - hand bone 7|
along the record, clamped to [0, length] (b62 said 0.68: corrected).

**Change.** skate-core `move_object::held_update`: `RebindTuning` (0.4, 0.04, 0.5, 1e-4, 60), `RebindState` (`tick`,
`decide` -> Refresh / Regrab / Lost), `hand_points`, `FrameBlend` (start / step), `seed_anchor`; `move_object::can_regrab`
(82E08DB8 with the grabbing box and the end exclusion). skate-game: `CarrierSkeleton` carries the hand span and the
collision flag (`solve.rs`); `PropCarry::hold` runs the block for an authored record held since last tick, re-grabs
through `regrab_candidate`, reseeds the follow anchor, and keeps the hand points.

**NOT RETAIL YET.** The push latch input (Player+736) is not identified (never set); path B's candidates are the held
prop's own records (retail: the owner's validated records), owner flag 0x40 and the held record's +196 / +216 are taken
as set (the hand IK on the hand points: step 3c below); the frame
blend is ported in skate-core but not wired (it only fires for 60 m jumps).

**Verification.** skate-core `held_update` tests (hand points and clamps, every rebind gate incl. the timer and the
latch, snap vs blend, anchor seed); skate-game prop / carry / skitch / living_world 136 pass. Not play-tested.

## Move Object step 3c: the hand IK (opt-in, 2026-10-10)

**Retail [code + data] (decoded by main in the TU3 recomp, 82D46610 at 82D469C8..82D46C9C; consumer 82D45008 from
research b64).** The held update's hand IK bit (+1200 0x40) is set in 82D46610, never cleared there; only 82D444A0
full (a grab, or a path B re-grab) clears it (0xC0). The set needs all three:
- the enter value +1180 (`5E35DB02BE697A58`, 0.7216) above 0;
- the state time (Player+2664) at most the larger x end (`bounds[2]`) of the curves `1348E9A1F213B42D` (x 0.5..1.0)
  and `702F25BA3A5AAA56` (x 0..0.35), i.e. 1.0 s with the stock data;
- 1 - curve `702F25BA3A5AAA56`(state time) above 0.1 (0x820641A8, 0.1): with the stock curve from about 0.1 s.

So the hands go to IK about 0.1 s into Move Object and stay on; a re-grab after the first second leaves them off. 82D45008
moves the weight +1132 by 0.2 per tick toward 1 (on) or 0 (off) and calls 82BD9728 / 82BD97D0 (hand A / B) with the
reach 0.65 (0x820BB0EC) and the targets +560 / +576 (82D46610 from the hand points +496 / +544).

**Change.** skate-core `move_object::HandIk` (`tick`: the gate then the weight step; `begin_grab`), `MoveObjectTuning`
`hand_ik_enter`, `hand_ik_window`, `hand_ik_curve`, `hand_ik_threshold`, `hand_ik_rate`, `hand_ik_reach` (loaded from the
attribute collection: the enter value, curve 702F and the window from both curves' bounds). skate-game: `CarrierSkeleton`
carries the state time (`state_timer_2664`), `PropCarry` steps the hand IK after the hand points (cleared on a re-grab and
on let go), and the solve phase writes the two hand points as IK targets into the handplant hand slots (limbs 2 / 3),
clamped to the reach around the animated hand targets as 82BD9728 does. Mod: `sdk.world.set_tuning('carry', {
hand_ik_enter, hand_ik_curve, hand_ik_rate, hand_ik_reach })` (enter 0 = the hands never go to IK).

**NOT RETAIL YET / open.** The targets are the hand points in world space (retail goes through the frame-local +480 /
+528 and the frame blend, which only differs during a 60 m blend); hand A = p+ (grip + half span), as retail's slot order;
a mod's curve does not move the window (it stays the disc curves' end). Only props with authored grab splines
(`SKATE_PROP_GRAB=1`) have hand points.

**Verification.** skate-core `hand_ik_turns_on_in_the_enter_window_and_stays_until_the_next_grab`, move_object 24;
skate-mods 105; skate-game prop / carry / world_tuning / modding / skitch / handplant 122. Not play-tested (grab a prop
with `SKATE_PROP_GRAB=1`: the hands should settle onto the edge).
