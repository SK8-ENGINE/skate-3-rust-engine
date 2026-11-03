# Ragdoll contact response vs the bail-sound gap: research spec (2026-10-03, research only, no engine change)

Question asked: retail's per-step Δv along the contact normal (p99 1.8–13 m/s, max 7.6–20 m/s, one-step "contact
stops") vs ours (~2 m/s peak, "lands and slides"). Find retail's ragdoll contact mechanism, compare with ours, say what
would have to change. Sources: the skate3recomp TU3 (title update 3) lifted code (reference only, our own words), recomp traces from our
own scripted runs, our code (read only). Credit: skate3recomp by @mchughalex, rexglue SDK, Xenia.

## TL;DR (what the user decides on)
1. **The physics gap is mostly a measurement artefact.** The retail Δv figures came from the scripted runs at the
   Super-Ultra Mega-Park spawn, where the skater fell ~2 s into the gap (or slid down the MegaRamp roll-in) and hit at
   **11–23 m/s**. In ordinary-height tumbles (first 2 s of the bails in `bailx3_20261003_110208`) retail's per-step
   |Δv·n| is p90 2.3 / p99 3.7 / max 3.7 m/s with approach speeds ≥ −4.3 m/s: the same size as ours (~2 m/s).
   Retail's large "one-step stops" are 1–3-step stops of 15–20 m/s impacts, removing 70–100 % of the normal velocity per
   step: what restitution 0 in the shared contact solver does, and what ours (a port of the same solver) also does.
2. **The bail-sound gap is audio-side: a missing speed curve.** The audio bridge `sub_824B0DA8`, after copying the
   conditioner's region impacts into the audio state (`+496..+524`, 8 regions), multiplies each by an 8-point graph
   (`sub_82481E10`) evaluated at the PREVIOUS frame's |COM velocity| (`+216` ← `+212`). The graph (read from the running
   game, watch `[[[0x830CFDA4]+36]+4]`): x = 0, 0.366, 0.448, 0.521, 0.593, 0.741, 0.863, 0.946; y = 1.0, 1.2, 1.486,
   1.914, 2.429, 3.6, 4.571, 5.0. So any bail moving faster than ~0.95 m/s gets ×5. Proven with the new `BANDQ` hook:
   the body poster's impact = 5.0 × the clamped region impact (p50 of the ratio 5.0 over 218 posts; 4.106 = 5 × 0.821,
   1.033 = 5 × 0.207, the 0.001 floor → 0.005). Our `skate_events::region_impacts` → `body_impact` skips this, so our
   impacts are 1/5 of retail's at the poster. Our max 0.21 per bail × 5 = 1.05 → tier 2 for every body material (bands
   0.25–0.65), and tier 1 / 2 on concrete (bands 1.0 / 1.85) become reachable: exactly the "concrete 1047 / 991 from a
   different impact" open question in doc 11.
3. Recommendation: **fix the audio side first (the curve), keep the physics as is**, then re-measure the bail density
   against the recomp before considering any physics change.

## 1. Retail mechanism (recomp code + traces)
### 1.1 The solve
- The ragdoll is solved in the same `rw::physics::Simulation` as the board: Horus iterative solver (`82AE27D0`, contact
  iteration `82AE2914..2B8C`), contacts built by ContactBatchBuild `82AE10C8` from the collision-to-contact writers
  (`8277A828` / `8277BFAC`). Contact rows solve velocity + position error together, a 4th lane position-only; normal
  impulse clamped to [0, ∞), friction cone static → dynamic. **50 iterations** (Simulation +176, doc 12).
- Skeleton world query: its own query record (`82BE5094` → `82768728` with edge threshold −1); the ground job's shared
  values (volume padding 0.05, max separating distance 0.5; speculative contacts).
- 60 Hz fixed step: the recomp's BAILSTEP shows 16.6 ms steps and SkeletonState dt 0.01667 always.
### 1.2 Materials, masses, damping (stock vault, `physics_skeleton` / `physics_wipeout`)
- Normal (riding / driven) collision: friction 0, restitution 0 (`FrictionNormal` / `RestitutionNormal`).
- Ragdoll (requests 8–11 → modes 7–10, `82BE6D60`): `FrictionRagdoll` 0.9 / `RestitutionRagdoll` 0 (head / hands the
  same values), mode 8 overrides all bones to 0.5 / 0.3 / 0.4 (static, dynamic, restitution). Contact combine: friction =
  max of the two, restitution = min (`82763078`) → a ragdoll part on any surface has restitution 0.
- `DoInverseMass` / `DoInverseInertia` true (ragdoll masses from the skeleton definition), `LinearDrag` 0,
  `AngularDrag` 0.5 × 60. `CollisionDriveScalar` 1.0.
- The audio "mass" SS `+4560` (0.0003–0.0098) is the bone-box volume used only for the impact formula, not the body
  mass; identical to ours (doc 11, masses section).
### 1.3 How velocities change at contact
- Restitution 0 → the solver drives the normal relative velocity to ~0 (plus position correction). Measured in the
  MegaRamp impacts: v·n −23 → −12 → −1 (foot), −17 → −3 (leg), −19 → −1.8 (arm): 1–3 steps.
- Bail-specific velocity writes (all ported in ours): Wipeout response trigger `82D3E678` (adds normal × control ×
  strength to all 23 parts), the two material responses `82D91708..82D92440` (`contact_response.rs`), retained velocity
  `82D3E348/82D3E8C0`; wipeout drives (`physics_skeleton_drives` per part, root scalars, `TimeToRemoveDrives`,
  `DynamicDriveWeightVel*`).
### 1.4 What the audio sees
- `82BEBD28`: SkeletonState velocities = finite differences × 60 of the part frames (body COM ⊗ local mass frame,
  `82585CB0`), Δv = v − v_prev (ours: `SkeletonPhysicalRecord::update`, identical).
- `82BD60C8`: region impact = clamp01(max(0.001, |Δv·n| × mass × 10)) → Collision `+80 + 4i` (8 regions; note retail
  takes |Δv·n|, ours the signed value; doc 11 masses section).
- `82773298` (conditioner): 4-frame ring, per-region max → block `+92` → skater entry `+320` → audio state `+496`.
- **`824B0DA8` (bridge): `+496 + 4i` ×= graph(`+216`), i = 0..7; then `+216` := `+212` (|COM v|).** ← missing in ours.
- Body poster `824BC188`: per region 0..5 with impact > 0 and no cooldown, tier = `82497088(material, impact)`
  (bands: tier 2 > `+0`, tier 1 > `+8`, none < `+12`; our exported bands match the game's exactly, BANDQ: 97 = 0.0025 /
  0.05 / 0.375 / 0.65, 98 = 0.0025 / 0.1 / 0.25 / 0.45, 99 = 0.0025 / 0.1 / 0.35 / 0.6, 100 = 0.005 / 0.2 / 0.65 / 1.25,
  2 / 3 / 65 = 0.12 / 1.0 / 1.85 / 2.0, 40 = 0.01 / 0.15 / 0.55 / 1.25). Tier 2 adds a second message at tier 1.

## 2. Ours (read only)
- `crates/skate-core/src/physics/{board_step.rs, solver/*, contact*.rs}`: the same Horus solver and contact builder,
  skeleton parts as attached bodies in the board's solve (`skate-game/src/physics/solve.rs`), 50 iterations.
- Skeleton world volumes `skate-game/src/physics/skeleton_colliders.rs`, query `board_world.rs::query_primitives` with
  the board's `query_settings()` (padding 0.05, separating distance 0.5) and edge threshold −1 for the skeleton.
- Materials / drag / masses / joint limits from the same vault fields (`wipeout_states/ragdoll/settings.rs`,
  `skeleton_body/collision_mode.rs`), wipeout responses and drives ported (`skate-core/src/player/wipeout_state/`).
- Record and region impact: `skeleton_body/record.rs`, `game_audio/skate_events.rs::region_impacts` → `body_impact`
  (4-frame ring) → `skate_audio::player::contacts::body`. **No speed graph.**
- No code-level difference found in the contact response. Not verified empirically: our per-step v_old·n distribution
  in a matched bail (no per-step skeleton log exists in our engine; adding one is a code change).

## 3. What would have to change
A. **Audio (recommended, small, skate-game / skate-audio only, no unsafe, skate-core untouched):** in the audio-state
   build, multiply each region impact (after the 4-frame max) by the bridge graph at the previous frame's |COM v|. Data:
   the 8-point graph (find its vault record: the global at `0x830CFDA4` +36 → +4 points at an attribute record; or stage the
   values with the other player tuning). Interpolation = `sub_82481E10` (already ported in `player/footsteps.rs`).
   Also the |Δv·n| abs (doc 11 masses section) and the 30 fps cadence of `Contacts::body`'s cooldown (doc 11 open item 3).
B. **Physics (not recommended now):** nothing identified that differs from retail. Only consider after A, if a matched
   comparison (same drop in both engines, our per-step log) still shows a gap.

## 4. Expected effect (A)
- Bails gain tier-1 / tier-2 body hits (1030–1032, 948 / 950 / 951 / 953 / 957) and concrete tier 1 / 2 (1047 / 991):
  louder, punchier impacts, closer to the recomp's 208 Collisions voices / Σg² 4.7 per bail (ours 46 / 1.5–1.9).
  Feel / look of bails: unchanged (audio only).
- Risks: louder bails than the user expects (retail parity says this is right; verify against the recomp capture and
  ask the user to listen); every slow body contact while moving (> 1 m/s) also ×5 → more tier-0 posts at the floor
  (0.001 × 5 = 0.005 ≥ the arm floor 0.005, ≥ 0.0025 for the others): retail does the same (BANDQ shows thousands of floor
  queries per run, mostly tier 3 for arms); the cooldown limits posts. `+212` = |COM v| in m/s is our port's reading of
  the state; the curve's x range (0–0.95) means "moving at all", UNCERTAIN whether retail's `+212` is in m/s; the
  measured ×5 at running / falling speeds and ~×1 at rest agree with that reading.
- Optimisation / regression: e2e bail scenarios change (expected), everything else must stay byte-identical.

## 5. Decision points for the user
1. Accept that the ragdoll physics is not the cause (no physics change): yes / no.
2. Port the bridge's speed graph on the region impacts (audio only): yes / no.
3. Together with it: the |Δv·n| abs and the 30 fps body-poster cadence (both already documented): yes / no.
4. If wanted later: a per-step skeleton contact log in our engine (diagnostic, behind an env var) for a matched-drop
   comparison with the recomp.

## 6. Evidence / tools
- Runs (0 malformed lines): `bandq_20261003_105210` (BANDQ, first build), `bailx_20261003_105626` (curve watch
  `impact_curve.watch`), `bailx3_20261003_110208` (6 / 6 bails with `lt rt l3 r3`); older
  MegaRamp-gap runs `bailrun_t2/ok/ok2/ok3`.
- Local scripts: `stop_anatomy.py` (large steps: time, run, first touch), `dv_window.py` (tumble vs later),
  `tier_thresholds.py`, `vault_named.py` (vault fields by name).
- Recomp hook `BANDQ` (`hooks_audio.cpp`, category `audiox`, 7 fields) on `sub_82497088`.
