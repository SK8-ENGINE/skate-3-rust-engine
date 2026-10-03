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
  The value is 50 before the first gameplay solve. The later writer is not identified yet.
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

## Open questions

- `customiser_equipment_reaches_ground_force_and_torque` (ignored test, needs private assets)
  fails at 50 iterations: "Profile hardness was lost before live straighten torque".
  - Samples of (side force, deck torque y) for hardness 0 / 0.7 / 1.0:
    - 25 iterations: (−23.31, −1.114), (−15.15, −1.114), (−11.66, +1.170)
    - 50 iterations: (−23.31, −1.114), (−15.15, −1.114), (−11.66, −1.114)
  - Side friction still scales with hardness exactly.
  - Even at 25, only hardness 1.0 changes the torque. The assertion appears to depend on a branch
    threshold (82C07000 limits the straighten request by the deck's existing angular velocity) in
    the test's synthetic one-frame state. This is not proven; the test is unchanged.
- The code that raises the count from 25 to 50 is not identified yet.
- Pops at 5–8 m/s still use the soft row. Our pushes reach 6–8 m/s where retail reaches about
  4 m/s. That is a separate physics issue.
