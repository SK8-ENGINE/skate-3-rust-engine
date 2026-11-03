# Board solver: 50 constraint iterations (retail live value)

## Problem

Ollie pops were too soft. Retail's player audio chooses the pop's loudness row from the
skater's jump velocity (audio state +468 = |Air+112| / 2.65, thresholds 0.25 / 0.42). A rolling
ollie in retail measures 0.50 at about 4 m/s. Ours measured 0.21–0.29, so every rolling pop used
the quiet row. A standing ollie (0.507) was already right.

## Root cause

The board/skater solve ran with `physics/default.RWMaxIterations` from the stock collections,
which is **25**. Retail's live `rw::physics::Simulation` solves with **50** iterations.

- The solver stages `82AE30F0` / `82AE27D0` read the Simulation's iteration count (+176) as their
  loop count.
- Setup (`82DC2840`) first copies 25 from a hard-coded config built by `8275DCC8` (`li r27,25`).
  The value is 50 before the first gameplay solve.
- The writer that sets 50: `82763E00`, called from `82859E70` for two owners, stores 50 into
  Simulation+176 when the owner's mode word (+8) is 3 and 25 otherwise. Traced on 2026-10-04 (see
  "Open questions"): mode 3 and 50 on every frame of gameplay. The Simulation constructor `82AE4A30`
  also defaults +176 to 50 (and +168 to 30).
- `RWMaxIterations` = 25 equals the setup value, not the live one.

With 25 iterations the constraints don't converge as far each tick, and the effects were
measurable:
- At rest, wheels sank about 10× faster than retail (−0.012 vs −0.001 m/s).
- By the pop's 4th frame the ollie deck was about 1 cm low.
- The jump velocity that the audio state reads was lower.

## Evidence

- TU3 traces in the static recompilation (reference only, not shipped): hooks on the Simulation
  setup and the solver entry logged the iteration count on every solve, recorded in Aletown with
  the board + skater pipeline (28 joints / 53 drives).
- Jump velocity after the change: a rolling ollie at 0.35–0.55 (retail 0.50 at ~4 m/s), standing
  0.56.

## Change

`crates/skate-game/src/physics/settings.rs`: `SIMULATION_ITERATIONS = 50` replaces the
`RWMaxIterations` read. The "zero iterations" validation goes away with it, since the value is
now a constant.

## Verification

- `--validate-maps` on every map, 25 vs 50: identical (collision triangles, support drop, grounded
  ticks, floor gap).
- Water drop traces (`water_drop`, ignored test), positions as expected with both counts:
  - University shallow channel (340.3, 69.94, −294.3): lowest body part 67.996–68.014 on 67.94,
    board 68.03. Settled mean part speed 0.059 → 0.120 m/s; peak 0.46 → 1.56. The user saw
    nothing wrong in play.
  - DownTown Aletown canal (−182.3, 10.93, 465.9): parts 8.3–9.0 vs 8.93, board 8.91, camera
    11.934. Settled 0.357 → 0.247 m/s.
  - The differences come only from the iteration count: a build with every other change but 25
    iterations reproduced the old numbers exactly.
- In play (user, 2026-10-02): pops, landings and grinds much closer to retail; nothing wrong seen
  in a shallow-water wipeout.

## Test fix: `customiser_equipment_reaches_ground_force_and_torque`

This ignored test needs the private assets. At 50 iterations it failed with "Profile hardness was
lost before live straighten torque". The cause was the test's setup. The solver and the customiser
path were fine.

**Root cause.** The test sampled the board one tick after spawn. At that tick the board is still
falling onto the floor: no wheel contacts, physical state 0 (not yet Ground), falling at
−0.16 m/s. The first solve also gives the deck some yaw: 0.310 rad/s at 25 iterations and
0.376 rad/s at 50. The test then injects sideways travel and runs the Ground update. The
straighten request goes through `82C07000`, which first subtracts the displacement the deck's
existing angular velocity already supplies along the request axis (ω·step). If the remainder points
against the request, nothing is applied. With the leftover yaw from spawn:

| iterations | existing ω_y·step | request at hardness 0.7 | request at 1.0 | applied |
|---|---|---|---|---|
| 25 | 0.00516 | 0.00291 | 0.00580 | only hardness 1.0 (margin 0.00063) |
| 50 | 0.00626 | 0.00291 | 0.00580 | none |

So even at 25 the assertion passed only by that small margin, and hardness 0.7 was already blocked.
The request itself was identical at 25 and 50. The difference is only the spawn-drop yaw, which is
a transient of the spawn, not something you see while riding.

**Evidence.** Diagnostic prints (removed again) around `apply_vector_82c07000`, plus a per-tick
trace of the settle. With 50 iterations the board lands about tick 8–9 (4 wheels by tick 9). It is
in Ground (state 100) at rest by about tick 20, with deck ω_y about 4.6e-5 rad/s from then on.

**Change (test only).** `crates/skate-game/src/tests/physics_startup.rs`: each run ticks 60 times
with the same default profile, so the board settles. Then the test applies the hardness profile
(the same call the customiser makes) and runs one more tick, so PlayerInput publishes it. Before
injecting the sideways travel it asserts the realistic precondition: Ground state, 4 wheel
contacts, |deck ω_y| < 0.01 rad/s. The intent is unchanged: the same injected travel, the real
Ground adapter, side friction = lerp(SoftestWheelSideFrictionScalar, 1, hardness), and hardness
must reach the deck torque.

**Verification.** Samples of (side force, deck torque y) for hardness 0 / 0.7 / 1.0:
- 50 iterations: (−21.142, −1.114), (−13.742, +8.378), (−10.571, +17.813). Friction ratios are
  exactly 2.0 / 1.3, and the straighten torque grows with hardness.
- 25 iterations (temporary constant, reverted): (−22.048, −1.114), (−14.331, +8.785),
  (−11.024, +18.621). The test passes too. It no longer hangs on a threshold.

## Open questions

- **Answered (2026-10-04): does retail ever drop to 25 in gameplay? No.** Which owner mode (+8) is 3?
  - **Where the mode comes from (static reading).** The world setup `8275DCC8` builds both owners with
    the constructor `827632A8`, which stores its 3rd argument as the mode word. The mode is chosen once,
    at construction, from the global byte `0x83082929`:

    | owner | Simulation | byte = 0 | byte ≠ 0 |
    |---|---|---|---|
    | slot 0 | setup owner +36 (the board + skater world) | mode 3 → 50 | mode 1 → 25 |
    | slot 1 | setup owner +40 | mode 0 → 25 | mode 2 → 25, plus extra calls on owner +21960 |

    `82763E00` only applies that fixed mode again on each frame. One writer of the byte stores 1
    (`826E5E78`, reached from `826E2A10`), another stores a register value (`826DA6C8`). What the byte
    means is still unknown. In every trace it was 0.
  - **Traces (the recomp, Super-Ultra Mega-Park spawn, background, muted).** The new `ITERSET` /
    `ITERTICK` hooks (category `physics`) were used, plus a watch list for the byte, both owners' modes
    and both Simulations' +176. 0 malformed lines. Data is in a local data folder.
    - `survey.tsv` covers riding, a forced bail on the board (both sticks + both triggers), the
      session-marker respawn, getting off the board and walking, a forced ledge bail on foot (FIRSTHIT
      +676 confirmed both bails), another respawn, getting back on the board, Start (opens the
      Career > Main menu) and Back (Instant Replay).
    - `survey2.tsv` covers riding, the Replay Editor (help, view, the exit dialog) and riding again
      afterwards.
    - Only slot 0 is ever written (lr `8285A1E8`, about 60 calls a second). It is mode 3 on every call.
      The count goes 25 → 50 at the first gameplay frame, then stays 50 the whole time: riding, both
      bails, walking, respawns, and after the menu and the replay.
  - **Menus and replay.** The Start menu and Instant Replay / the Replay Editor stop the frame update
    `82859E70` entirely (no `ITERTICK`). So there is no solve and no write there, and the count is
    still 50 when gameplay resumes.
  - **Slot 1.** The owner stays mode 0 and its Simulation stays 25, but `82763E00` is never called for
    it. The solver stage never ran with 25: `SOLV30F0` logs each new (partition, count) pair, and it
    logged only count 50.
  - **Not covered.** Party Play, online play and Free Play from the menu (the scripted menu path did not
    reach it). They would matter only if they rebuild the world with the byte set.
  - **For the port:** a constant 50 is right for normal gameplay. If one is ever needed, the 25 case
    belongs to whatever sets the byte, not to off-board play, bails, menus or replays.
- Pops at 5–8 m/s still use the soft row. Our pushes reach 6–8 m/s where retail reaches about
  4 m/s. That is a separate physics issue.
