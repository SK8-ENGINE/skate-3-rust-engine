# 17: Offboard jump crash: "Nonfinite BipedAir launch packet" (upstream issue #10)

Branch: `fix/offboard-jump-nan` (from `main` 4488651). Upstream issue:
SK8-ENGINE/skate-3-rust-engine#10, reported by @NoahVl with a first-divergence trace.

## Problem

After a few minutes of walking offboard, a jump exits the game:

```
BipedAir launch producer rejected ... secondary_velocity_16: [0.886, 0.0, -0.464, NaN],
position_32: [-153.8, -2.81, -58.6, NaN] ... error=Nonfinite BipedAir launch packet
```

Every XYZ lane is finite and plausible; only the W lanes are NaN. Shortly before, `OFFBOARD_GROUND_RESULT_NONFINITE`
is logged every tick, with W = NaN in `frame_0[3]`, the support velocities (`support_velocity_256`,
`predicted_support_velocity_272`, `previous_support_velocity_288`, `support_acceleration_304`) and
`collision_displacements`.

## Root cause

An unstable feedback loop on the W lane of the BipedGround frame position, in
`ground_motion::support::update` (82D7EEAC..F454) and `ground_motion::update` (82D7EDC8):

1. `moved = transform_point(frame_0[3], delta)`: `transform_point` is homogeneous. It ignores the point's W and
   builds the output W from the frame's W lanes (0 for host frames).
2. `velocity = (moved - frame_0[3]) / DT` is four-lane, so `velocity.W = -frame_0[3].W / DT`.
3. `predicted_support_velocity_272 = 2·v − v_prev`, and `ground_motion::update` integrates it back:
   `frame_0[3] += predicted · DT`.

For W alone that gives `W[t+1] = W[t-1] − W[t]`. The roots of that recurrence are 0.618 and **−1.618**, so any non-zero
W in the frame position grows by about ×1.6 a tick with alternating sign. It happens on every supported tick, standing
or walking (static support counts: same support id, so the derivative branch runs). A 1e-3 seed reaches Inf after
about 195 ticks (~3 s) and becomes NaN on the next tick (`Inf − Inf`). The NaN then sticks in `frame_0[3]` and spreads
to the ground result, the COM (`centre_of_mass_1056 = result.position`), the skeleton extra targets and so
`collision_extra_errors` (which is why the reporter saw it there first). On the next jump it reaches the launch packet,
which `air_selector::candidates::prepare` correctly rejects.

XYZ are never affected. Every consumer of these vectors that mixes lanes uses `dot3` / XYZ-only maths, which is why the
packet looks fine apart from W.

### What seeds it

The loop only needs one non-zero W. On flat test ground nothing seeds it (a 3000-tick headless walk with jumps, a
bail and recovery kept every W at exactly 0). In play, W lanes that carry native permute scratch by design can seed it:

- animation-directed moves: `target_frame_608[3] = row0·0.2 + row3` takes the authored target frame's W lanes, and
  the step `target − position` carries them into `frame_0[3]`;
- skeleton extra errors (`pose[24+i][3] − targets[1+i]`) through `ContactCorrection` when it activates. This is the
  path the PR #9 commit closes.

We could not determine which seed fired in the reported session, because the trace's first line was truncated. But
the loop amplifies any seed, which is why the crash appears seconds after a seemingly unrelated event (here a bail
respawn plus two jumps).

## Evidence

- Unit probe (skate-core, `ground_motion` tests): supported, static, same support id, one 1e-3 W kick in
  `contact_displacement_384`. W per 20 ticks: 0 → −0.99 → −1.5e4 → −2.3e8 → … → Inf at tick 195. XYZ stayed exactly
  `[1, 0, 2]` the whole time. This matches the issue: finite XYZ, NaN W, NaN support velocities.
- Headless asset-backed walk (stock assets, 3000 ticks, circling stick, jumps, one wipeout/recovery): every observed
  W (extra errors, targets, COM, `animation_to_world`, `com_frame`, `frame_0`) stays exactly 0. So the bug needs a seed,
  and the clean path cannot show it.

## Change

`crates/skate-core/src/player/offboard/ground_motion/support.rs`: the derived support velocity keeps the host's geometric
W = 0 convention (as `math::rotate` already does): `velocity[3] = 0.0` after the four-lane difference. That lane was
never a velocity component: it came from mixing a homogeneous `transform_point` with a four-lane subtraction. Zeroing it
breaks the loop at its source, and any seed W now stays bounded (constant, or damped by the contact correction).

Why this is not "clamping NaNs away":
- it sets a lane that is meaningless by construction, before any NaN can exist, and it changes no finite XYZ value;
- the launch packet validation (`candidates::prepare`) stays untouched and still fails loudly on real non-finite state.

Behaviour identity for gameplay: `velocity` W only reaches other W lanes (acceleration, predicted velocity,
`velocity_480`). Every XYZ result is bit-identical: `dot`, `length`, `normalize`, `transform_vector` and `cross` build
XYZ from XYZ only, and `filter` projects through columns with W = 0. On the clean path W was already 0, so nothing
changes there at all.

## Files

- `crates/skate-core/src/player/offboard/ground_motion/support.rs`: the fix (one lane, with comment).
- `crates/skate-core/src/player/offboard/ground_motion/tests.rs`: two regression tests:
  - `stray_position_w_is_not_amplified_by_support_velocity`: a 1e-3 W kick, 600 supported ticks; support velocity W
    stays 0, frame W stays bounded, XYZ unchanged;
  - `animation_target_w_scratch_cannot_blow_up_the_ground_frame`: seed via an animation-directed target frame with
    native scratch W (each row's X repeated into W), 600 ticks, frame stays finite and bounded.

  Both fail without the fix (frame W reaches Inf / exceeds bounds) and pass with it.

## Verification

- `cargo test --locked --release -p skate-core`: 618 passed, 2 failed. Both failures are the known pre-existing upstream
  ones (`predictive_contacts_and_retention_match_full_scan_for_every_primitive`,
  `a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`).
- `cargo test --locked --release -p skate-game --bin skate3rust`: 309 passed, 2 failed, 101 ignored. Both failures are
  known and pre-existing (`pipelines_accept_valid_group_outputs_when_fingerprint_changes`, `sky_shader_validates`).
- All 19 asset-backed offboard playback tests (`-- --ignored offboard`, stock assets): pass. They cover jumps with the
  board held or thrown, walk-off-edge, midair dismounts, recall/remount and walk-turn loops.
- `check_spawns.py`: all maps ok (a physics-only change, as expected). `check_maps` needs `--validate-maps`, which
  `main` lacks, so it was skipped.
- Not play-tested: the crash needs a seed that only appears in real sessions. The unit tests cover the mechanism.

## Prior work and relation to other fixes

- Upstream PR #9 (laaledesiempre, `linux-port`, commit a01e414; also carried in PR #15) zeroes W where the skeleton
  pose errors are produced (`errors.rs`) and on `ContactCorrection`'s outputs. That removes one seed but leaves the
  amplifier, so any other W seed (for example animation-directed targets) can still crash. The two fixes are
  complementary and do not conflict.
- Upstream PR #4 (andrewnakas, commit 4e65ecb) makes the launch packet / trajectory validators check XYZ only. That
  removes the symptom, and the W lane would still run to NaN silently.

## Open questions

- Which seed fired in the reported session (the trace's first line was truncated). A trace that logs the first
  non-zero `frame_0[3].W`, not only the first non-finite value, would answer this.
- Retail VMX (the Xbox 360 vector unit) code does the same four-lane maths, so retail W lanes may well drift to Inf/NaN too, harmlessly, because
  retail never validates W. The port's W = 0 host convention is a deliberate host choice, not measured retail behaviour.
