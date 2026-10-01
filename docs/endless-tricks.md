# Endless Tricks: the flip ladder past retail's quad

**This is a deliberate extension beyond Skate 3, not a port fix.** Everything else in this engine
is a faithful reimplementation of retail TU3; this is new work built on top of it, shipped as a
mod, and it is off unless that mod turns it on. With it off, the ladder, the names, the points and
the animations are exactly what the game shipped with — see *Proving it stays retail-exact* below.

Built 2026-09-24. First entry in the Endless Tricks mod; the mechanism generalises to other trick
families.

## What retail actually does, and why nothing in code could be uncapped

The flip ladder is **authored, not computed**. There is no flip-count variable and no clamp to 4
anywhere in the Rust. `T_Kickflip.xml` authors `TrickInto -> Cyc1 -> Cyc2 -> Cyc3 -> Out4` and
simply has no `Cyc4`.

Two facts settle the shape of the problem, both read straight out of the asset tree:

* `T_Kickflip.xml:247-251` — `Cyc3`'s only outgoing transition is a bare
  `WillExpire InTime="0.05"` to `End.Out.Out4`, with no precondition.
* `End.Out.Out4`'s state definition carries **no `<expression>` at all**, unlike `Out1`/`Out2`/
  `Out3`, whose `not HasIntent <Trick>Hold` / `TimeToLand` / `IsBodyFlipping` gates end the ladder
  early. Rung 4 is unconditional.

So the quad is not a gate that can be relaxed. The extra rung has to be authored.

That was worth proving rather than assuming. Suppressing `WillExpire` on `_CYC3` behind a
throwaway env var made `B_KICKFLIP_OUT4`, `B_AIR_CYC` and `BLEND_LAND` all disappear from the
animation list and the reward drop to zero — the skater never left the state and never landed,
which is exactly what "the only exit" predicts. It also confirmed the clip does not loop of its own
accord: `clip_clock.rs:88-93` sets `crossed_end` on a non-looping clip and lets time run past
`length`, and `sample_time()` clamps the sample, so holding the state parks the deck on the last
frame.

## How the loop is made

`crates/skate-game/src/graph_host/endless_flip.rs` appends one transition per flip cycle state,
pointing the state **at itself**, guarded by a new `EndlessFlipLoop` condition. Installed between
`StateGraph::load` and `Binding::from_graph` in `graph_runtime.rs`, on the MotionGraph only.

Appending is safe because `Binding::from_graph` requires only `child > parent` and unique
ownership — **not contiguity** (`binding.rs:124`) — so new elements go on the end of the arena and
nothing renumbers. Binding, name resolution, compilation and condition instantiation then all run
through the ordinary authored path; there is no parallel-array surgery.

Three details that are easy to get wrong:

* **The self-target resolves for free.** `find_state(start = Cyc3, name = "Cyc3", ascend = true)`
  misses in the child loop and then matches the state itself on its first ancestor step.
* **Ordering needs one fixup.** `Binding::from_graph` walks elements in *index* order, so an
  appended transition lands last in its owner's list and `search_transitions` takes the first
  activatable — which would let the authored exit win every time. `reorder` swaps the final pair,
  putting the loop immediately ahead of the authored exit. Underflip and dark catch are earlier in
  the list and keep their authored priority, which is why an endless flip still cannot be
  underflipped past the fourth rung.
* **Re-entering a cycle state is authored retail behaviour, not an invention.**
  `T_Kickflip_Unique.xml` — the Mike Carroll and Gonzo signature kickflips — runs
  `TrickCyc1`/`TrickCyc2`/`TrickCyc3` over the *same* clip, differing only in the scoring name.

### The six install sites

`install` asserts it finds exactly six and fails graph load otherwise, because quietly installing
onto a changed asset tree would be worse than refusing.

| clip | quad `ScoringTrick` | authored exit |
|---|---|---|
| `B_KICKFLIP_CYC3` | `Kickflip4` | `End.Out.Out4` |
| `B_HEELFLIP_CYC3` | `Heelflip4` | `End.Out.Out4` |
| `B_N_KICKFLIP_CYC3` | `N_Kickflip4` | `End.Out.Out4` |
| `B_N_HEELFLIP_CYC3` | `N_Heelflip4` | `End.Out.Out4` |
| `B_KICKFLIP_MCARROLL_CYC` | `Kickflip4` | `TrickOut` |
| `B_KICKFLIP_GONZ_CYC` | `Kickflip4` | `TrickOut` |

Nollie comes free: it is the same authored file, included a third and fourth time.

## No height bar, by decision — 2026-09-25

The first build gated every rung on having the air to finish it: `time_to_land >= cycle * 2 +
blend`. That reads as a height bar, and for the single-clip families it is a brutal one, because
their "cycle" is the whole trick animation and so wants roughly twice the air a kickflip does.
That is why endless 360 flips were effectively unreachable in play while laserflips occasionally
landed.

**The default is now permissive**: hold the input, stay off the ground, keep flipping. The only
requirement left is room to cross-fade. A flat pop reaches a `720 FLIP`.

The cost is real and worth stating: the ladder will start a rotation it cannot land out of, so
bails go up. `TrainerTuning::endless_air_check` (mod setting *Require air for each flip*, default
off) restores the conservative budget for anyone who would rather land than rotate.

Retail's own air-time gates on the first four kickflip rungs are untouched in both modes. What
changed is only whether *extra* rungs demand air up front.

## Repeats play the authored cycle clip

Reported from play, twice: holding a 360 flip made the skater re-pop the trick on every rotation,
and then made the back foot look glued to a jittery board.

Both came from the same mistake, and the mistake was a **research failure, not a design one**. I
searched the state graph for cycle content, found none, and concluded retail had authored none for
this family -- then spent two rounds patching around that: entering the clip 45% in to skip its
windup, and freezing the body range to stop it re-performing. The second patch is what glued the
foot to the deck, because the reparented helper bones (32-35) are expressed in the board's *moving*
space, so freezing them makes them ride the board.

**The cycle clips existed the whole time, in the animation bank, referenced by no authored state:**

| clip | loop-closure error |
|---|---|
| `T_360FLIP_H_CYC` | 0.21 |
| `T_360FLIP_L_CYC` | 0.20 |
| `T_LASERFLIP_H_CYC` | 0.23 |
| `360FLIP_D_HIGH_A` (the air clip, looped) | **10.56** |

The lesson worth keeping: *the graph not naming a clip does not mean the clip does not exist* --
search the bank, not only the authored states.

### ...but playing them was wrong, and the closure figure is why that was missed

> **Superseded.** This section used to end by saying a repeat swaps in `T_<trick>_H_CYC`, that
> `N_360Flip` is dropped, and that rotations are therefore slower. All three are false. They are kept
> here because the reason the closure table misled is worth more than the table.

Those clips are hold **poses**, not rotations. Measured, `T_360FLIP_H_CYC` moves the board a tenth as
much as the kickflip's cycle does -- which is exactly *why* it closes its own loop so tightly. The
0.21 against the air clip's 10.56 was measuring **stillness, not loopability**, and playing one on a
repeat parked the deck, so only the first rotation was ever visible.

So a rung keeps playing the trick's own air clip, which is the thing that turns the board, and
`animation_pose`'s body/board split replaces the body alone -- parked in `T_HI_KICK_CYC2`, the
kickflip's cycle, which retail authored for legs held clear of a rotating board. The shape is:

```
B_360FLIP_G -> B_360FLIP_A -> B_360FLIP_A (xN, body replaced, deck spun) -> BLEND_LAND
```

Consequences, corrected:

* **`N_360Flip` is not dropped.** It ladders. `scoring::extension` gives `n_360flip` (id 106) a full
  rung table, `has_ladder` asks that table rather than a hand-written list, and `gesture_input`
  includes `N_360Flip` among the gestures the mod lets you hold. `install` logs all twelve:
  `360FLIP 360HARDFLIP 360INWARDHEELFLIP 360POPSHUVIT FS360POPSHUVIT LASERFLIP` plus every `N_` form.
* **Rotations are not slower.** The air clip is what replays, so the pacing is the trick's own. Since
  the deck is now spun at its measured plateau rate rather than one turn per clip length, a held
  rotation is in fact *faster* than it was: measured 0.54 against 0.28 of board movement per tick.

### The held deck is spun, and every number in the spin is measured

The air clip turns the board, but replaying it replays its **acceleration**: a ramp off the flick, a
plateau, then the catch slowing the deck. So each rung visibly re-accelerated. Four attempts at
retiming the authored frames each traded that for something else -- entering at an "at speed" frame
only moved where the ramp sat, evening out the rate surged at the seam because the clip holds no whole
number of revolutions, and trimming to a window that closes chopped off visible motion.

The deck does not need resampling. Every bone of the board rotates by the same amount through the
clip -- `TRUCK_BACK` 494.3 degrees, the wheels and `SKATEBOARD_ROOT` the same to a tenth of a degree
-- so it is one rigid body, and one pose of it turned at a steady rate is all a hold needs.

`cargo run -p skate-data --example spin_measure -- <assets>` prints the numbers, in seconds, without
building the game. For `360FLIP_D_HIGH_A`:

| what | value | why it matters |
|---|---|---|
| path `sum abs r` | 494.3 deg | total angular travel through the clip |
| net `abs sum r` | 360.6 deg about `[-0.44, 0.72, -0.53]` | one clean revolution about an axis tilted **43.6 deg** from vertical |
| ratio | 0.729 | near `1/sqrt(2)`: the instantaneous axis wanders, as a flip-plus-shuvit does |
| endpoint axis vs accumulated | `dot = -0.60` | the old axis was **inverted** |
| plateau | 20.85 deg/frame | against a whole-clip mean of 15.45 |

Three things follow, and each was a bug:

* **Direction.** The axis came from `to_axis_angle` of the net first-to-last-frame rotation. A
  494-degree path is not recoverable from its endpoints, and the measurement shows that axis points
  backwards: `dot` is -0.60 on `_HIGH_A`, -0.76 on `_LOW_A`, -0.89 on `GONZ_HIGH_A`. That was "the
  board goes in the wrong direction", as a number. Summing the per-frame rotation *vectors* cannot
  invert, because a signed sum has no axis sign to guess.
* **Speed.** One turn per clip length is 654 deg/s where the plateau is 1251 -- nearly half. That is
  why every rung read as sluggish. The rate is now the mean of the fastest third of frames, which
  lands on the plateau and is immune to a single noisy frame.
* **Where the deck sits.** The rigid reference was frame 0, the deck still at the feet pre-flick, so
  a spinning deck was parked inside a body whose legs are held clear. It is now the **last** authored
  frame, which makes the hand-off out of the real trick continuous in orientation and position both,
  with the first synthesised rung starting at angle zero.

The 0.73 ratio means a fixed axis is an **idealisation**, chosen deliberately. Replaying the authored
directions instead carries 15-49 degrees per frame of direction wobble plus a 20-101 degree kink
where the sequence repeats; a fitted yaw-plus-flip model reconstructs the clip no better than 65
degrees. "Floaty" and "smooth" is what was asked for, and a steady turn about an axis tilted 44
degrees is still a kickflip combined with a shuvit -- that tilt *is* the trick's geometry.

The angle accumulates across rungs, so the step over a junction equals the step inside a rung and the
spin need not complete a whole number of turns per rung. There is no loop to close, which is what all
four resampling attempts broke on. Decay is a rate multiplier per rung, so it composes with that for
free.

### The knobs, none of which need a rebuild

Whether a held leg clears a rotating board is a judgement only play can make, so the pieces that are
judged by eye are switchable from the environment:

| variable | default | what it does |
|---|---|---|
| `SKATE_ENDLESS_RIGID_SPIN` | on | `0` goes back to replaying the authored board motion |
| `SKATE_ENDLESS_SPIN_RATE` | 1.0 | multiplier on the **measured** plateau rate |
| `SKATE_ENDLESS_SPIN_DECAY` | 0.97 | per-rung slowing; `0.97^11` takes the twelfth rung to 0.72 |
| `SKATE_ENDLESS_HOLD_CLIP` | `T_HI_KICK_CYC2` | the body-hold clip; a bare name or `<TRICK>=<clip>;...` |
| `SKATE_ENDLESS_HOLD_SWEEP` | 0.12 | how much of the hold clip a rung traverses; `0` freezes the pose |
| `SKATE_CLIP_TRACE` | off | `SKATE_CLIP` and `SKATE_SPIN` lines: the bank names, and the axis |

### The rung has to survive its own landing

Found by the test that came with this change, and it predates it: the ladder was cleared on the
first grounded tick, while the authored `ScoringTrick` leaf was still publishing. A five-rotation
360 flip therefore collapsed to `#ID_TRICK_FLIP_360_FLIP` and banked as a plain one, at the moment
it landed.

A grace period was the obvious fix and the wrong one -- it repaired the 360 flip and not the
laserflip, because how long that publishing tail runs depends on the clip. The ladder is now
cleared at **takeoff**: exact, because it always outlasts the landing it was earned on and the next
trick always starts clean.

## The air budget, and why it is not a constant

The loop is gated on the hold intent and on not body-flipping — the same terms the authored rungs
use — plus an air-time term. **The air-time term is measured off the clip, not picked**:

```
time_to_land >= current_clip_length * 2
```

one more cycle plus an out clip of comparable length. A fixed threshold does not work, and that was
found the hard way: an earlier version reused `End.Out.Out2`'s authored 0.8 s and the heelflip
started a cycle it could not land out of, ending in `B_WIPEOUT_FLAIL_GESTURES` with a zero reward.
The heelflip's clips are longer than the kickflip's, so the budget has to scale with them. **A rung
the skater cannot land out of is worse than no rung.**

The authored `Out1`/`Out2`/`Out3` preconditions are **not touched** in either mode. Note that the
budget described here is now the opt-in `endless_air_check` path; the default is the permissive one
described above.

## The loop junction has to be blended, and the blend is measured

Re-entering the cycle state cuts. `_CYC3` is authored as the **third clip of a sequence**: its end
pose is shaped to run into `_OUT4`, not back into its own first frame. So the first playable build
flailed -- arms snapping around once per extra flip, which is what a pose discontinuity looks like.

That was measurable rather than a matter of taste. Sampling every skeleton part relative to the
root each tick and taking the worst single-tick movement:

| | worst in-flip jolt | where |
|---|---|---|
| stock quad | 0.238 m | `B_KICKFLIP_IN_A`, the takeoff |
| endless, cut | **0.412 m** | `B_KICKFLIP_CYC3`, once per cycle |

The spikes landed at ticks 197, 223 and 249 -- 26 apart, exactly the cycle clip's length, so one
per loop.

The fix uses retail's own mechanism. The loop transition carries an
`OverideNextAnimTransitionHook` (`MotionHook::Override`, which sets `playback_context
.transition_override` and is consumed by `PlayAnimation::begin`) with `transType="blend"` and
`blendWithCurrentFrame="true"`, so the re-entered clip cross-fades out of the pose actually on
screen instead of cutting to frame zero.

**The blend length was swept, not chosen:**

| blend | worst in-flip jolt |
|---|---|
| 0.00 s | 0.334 m |
| 0.05 s | 0.238 m, but small cycle-clip spikes remain |
| **0.08 s** | **0.238 m, no cycle-clip spike at all** |
| 0.15 s | same as 0.08, and costs more air |

0.08 s is the shortest window where the roughest moment of an endless flip is an authored junction
again. `the_loop_junction_is_no_rougher_than_an_authored_one` pins both halves of that: the endless
worst must not exceed the stock worst, and no `_CYC` tick may be the roughest moment of the flip.

The blend is part of the cost of another rung, so it is part of the air budget too:
`time_to_land >= cycle * 2 + LOOP_BLEND_SECONDS`.

> A caution recorded because it nearly became a wrong fix: after adding the blend, a boost that had
> previously landed started wiping out, and the obvious reading was that the blend had eaten the
> landing margin. It had not. Running the **stock** ladder at the same boosts shows it banks zero
> there too -- past about 15 m/s the flat canonical course simply runs out. Landing success at that
> margin is chaotic and needs the mod-off control to interpret at all.

## Naming and scoring, without touching the retail tables

`catalog::IDENTIFIERS`, `conversions::LINKS` and `SCORABLE_COUNT` are recovered data and are
**unchanged**. No 333rd scorable id exists.

Instead `crates/skate-core/src/scoring/extension.rs` gives each extra rung a mod-only identifier
(`kickflip5` …) that **borrows the retail quad's ledger identity** — 135 for the kickflip, 134
heelflip, 137 nollie kickflip, 136 nollie heelflip — and overrides only the published name, the
label and the points.

That works because the ladder *converts* rather than accumulates (`Carrier::convert_to`): the only
things that ever reach `ScoreHolder` are the surviving carrier's `Scorable` and its reward, and the
quad's `Scorable` is already the right ledger identity for a fifth flip — same class 3, same score
type 2, same repetition bucket, same conversion source. So `Scorable::valid()` passes, `LINKS[135]`
is in range, and `by_id(135)` resolves. Minting a high id instead would have hit all three: an id
past 332 panics at the unchecked `LINKS[*new]`, and `ScoreHolder::end_trick` silently drops it, so
the rung would have been named correctly and scored nothing.

One thing had to change to make that work. `Runtime::carrier` keys the "is this a new trick" test
on the scorable id alone, so borrowing 135 for every rung would create no new carrier. A rung
ordinal now rides alongside (`Runtime::extension`), and it is **constant zero on the retail path**:
nothing publishes an extension name, so the added disjunct is permanently false and `points` and
`label` reduce to the definition's own.

**Named counts stop at the octuple.** Rungs 5 to 8 read `QUINTUPLE`, `SEXTUPLE`, `SEPTUPLE` and
`OCTUPLE`; from the ninth flip on the label is simply `ENDLESS KICKFLIP` (or `ENDLESS HEELFLIP`,
`ENDLESS NOLLIE KICKFLIP`). Past eight the exact count is not something a player is tracking in
the air, and a bare `12X` reads worse than the trick's own name. The rungs still differ in
identifier and points; only the displayed label repeats.

**Points continue retail's own step.** The authored ladder is 100 / 150 / 200 / 250 — a flat +50 a
rung — so a quintuple is 300 and the sixteenth flip is 850. Linear is deliberate: only the final rung banks,
and a superlinear curve would let one lucky drop dwarf a whole line.

**Labels are plain words, not `ID_TRICK_*` keys.** `apt_text::localize` splits a `#`-prefixed name
on whitespace and echoes any token it cannot resolve — that is how a spin's bare `360` survives —
so `QUINTUPLE KICKFLIP` renders as written and needs no language-table entry. An
`ID_TRICK_FLIP_QUINTUPLE_KICKFLIP`-style key would have reached the HUD as that raw identifier.

## Measured

`crates/skate-game/src/tests/flip_playback.rs`, flat course at 8 m/s, takeoff velocity boosted on
one frame. Flip counts are structural; the reward pair is at boost 14.

| takeoff boost | flips reached |
|---|---|
| 8 (the stock quad boost) | 4 — unchanged |
| 11 | 5 |
| 12 | 6 |
| 14 | 7 |
| 16 | 8 |
| 18+ | 9 — but the skater overshoots the flat course and never lands |

The ninth rung is where the counted names stop: at boost 18 the harness reaches
`Kickflip9` and the composed name is `#ENDLESS KICKFLIP`. It is pinned by
`the_ninth_flip_and_beyond_is_named_endless`, which asserts the *name* and the rung count
rather than a banked reward, because nine rungs needs more pop than the flat canonical
course has room to come back down from. The landing is covered at boost 14 and 16 instead.

At boost 14: the stock ladder banks **969.25** and names `#ID_TRICK_FLIP_QUADRUPLE_KICKFLIP`; the
same pop with the mod on banks **1119.22** and names `#SEPTUPLE KICKFLIP`. The difference is
exactly 150 — three rungs at the authored +50 — which also pins that the ladder still *converts*
rather than summing every rung beneath it.

## The 360 flip family, which is a different problem

Added 2026-09-25 for `360Flip`, `Laserflip` and their nollie forms. It works, and it is honestly
worse than the kickflip ladder, for reasons that are structural rather than fixable.

**Retail authors nothing to build on here.** `T_TrickWithDarkCatch.xml` has no cycle states at
all -- one `ScoringTrick`, one air clip -- and `360flip` (85) / `laserflip` (99) have no numbered
scorables. Where the kickflip ladder reuses purpose-built `_CYC` clips, every extra rotation here
repeats the **whole trick animation**.

**And the hold mechanic does not exist for it.** Only `Kickflip`, `Heelflip`, `N_Kickflip` and
`N_Heelflip` are hold-capable, and only `T_Kickflip.xml` and `T_Kickflip_Unique.xml` consume a
`<Trick>Hold` intent -- checked across the whole authored tree. So these gestures cannot be held
at all until the mod widens the set, which it does **behind the trainer flag**: `HoldPattern` is
read by the authored ActionGraph (`trick.xml`), so widening it ungated would change stock
behaviour for those gestures. The loop condition gates on `HoldPattern` for these families rather
than a `<Trick>Hold` intent that has no authored existence.

**Naming counts rotations, not repeats.** A doubled 360 flip is a 720, so the rungs read
`720 FLIP`, `1080 FLIP`, `1440 FLIP` … and `ENDLESS 360 FLIP` from the ninth. Laserflips read
`720 LASERFLIP` and so on. Points continue the same +50 step, counted from each family's own
authored rung rather than a number invented here -- which is why `Rung` carries `base_rung` and
points are computed by `points_over(retail_points, rung)`.

### Measured

| | stock worst jolt | endless worst jolt |
|---|---|---|
| kickflip | 0.238 m | 0.238 m — no rougher at all |
| 360 flip | 0.161–0.227 m | 0.312 m |
| laserflip | 0.202–0.268 m | 0.370 m |

About 1.4x rougher at the worst moment. Rewards at 14 m/s: 360 flip 995 -> 1303 (`#1800 FLIP`),
laserflip 903 -> 1205 (`#1800 LASERFLIP`).

> **A cliff sits next to the chosen blend.** Sweeping the junction cross-fade for this family:
> 0.02 s gives 0.861 m, 0.04 gives 0.474, 0.06 gives 0.457, **0.08 gives 0.312**, and then 0.10
> gives **12.7 m** and 0.16 gives **7.8 m**. The optimum is sharp and its neighbour is a blow-up.
> That instability is not understood. It means this family is fragile in a way the kickflip
> ladder is not, and a clip whose timing differs could land in the bad zone -- worth suspecting
> first if a 360 flip ever does something violent rather than merely choppy.

### Scope

`360Hardflip` and `360InwardHeelflip` are **in**, along with every other `_360`-family stem and all
their nollie forms — twelve in total, which `install` logs on load. There is no list of family names
anywhere: `has_ladder` asks `scoring::extension::by_family`, so a family earns a ladder exactly by
having rungs in the table and the two cannot drift apart. That is also what makes "a 180 trick can
never be endless" structural rather than remembered — `fspopshuvit` (90) and `n_fspopshuvit` (111) are
absent from the table on purpose, because a pop shuvit turns the board 180 and counting its rungs in
720s would be a lie.

(This section previously said those two were left out and pointed at a `SINGLE_CLIP_FAMILIES`
constant. Neither the exclusion nor the constant exists.)

Note also that each include carries a **second** `LeftGround` inside `GrindOutAssist` that plays
the same clip. Only the real air state sequences into it; the assist blends. Without that
distinction the site count comes out at fourteen rather than ten, which is how the strict
`EXPECTED_CYCLE_SITES` check earns its keep — it counts the six *cycle* sites, while the single-clip
families are discovered and logged rather than counted.

## Played, 2026-09-25 — University

Launched through `PLAY-ENDLESS-TRICKS.bat -Trace` against install
`84bb8943f4a0439a80a7cc5e2e4489ca`. **A different install from the one the tests use**, which is a
real check on the site discovery: `install` refuses to load the graph unless it finds exactly six
flip cycle states, so the game reaching University proves the predicate is not overfitted.

Over the session: **41 sequences published, 10 of them on an extension rung**, and rungs 5 to 8
all reached. The banked `points` land exactly on the authored +50 step -- 300, 350, 400 and 450
for rungs 5, 6, 7 and 8 -- alongside ordinary 100/150/200/250 kickflips from the same session,
so the stock ladder and the extension run side by side. Three of the publishes:

```
SCORE_PUBLISH reward=6224 mult=3.00 line=6841 bail=false name="#OCTUPLE KICKFLIP 180"
SCORE_PUBLISH reward=333  mult=1.50 line=333  bail=false name="#QUINTUPLE KICKFLIP"
SCORE_PUBLISH reward=0    mult=1.00 line=0    bail=true  name="#SEXTUPLE KICKFLIP"
```

Four things this confirms that the headless tests could not:

* **The label composes with a spin.** `#OCTUPLE KICKFLIP 180` — the extension rungs inherit the
  retail quad's `trick_type`, so `decorates_spin` treats them exactly like a quad and the degree
  count lands on the end of a mod-only label. That was a deliberate choice and it holds.
* **The points are the rung.** Banks of `points=300`, `350` and `450` are rungs 5, 6 and 8 on the
  authored +50 step, read straight off `SCORE_TRICK air ... id=135`.
* **The repetition penalty applies for free.** A repeated quintuple banked `points=300 reward=60.0`
  — factor 0.2, decaying down the authored curve exactly as a repeated quad does. This is the
  split-identity design paying off: borrowing id 135 means the rungs share the quad's repetition
  bucket, which a parallel scoring channel would have had to reimplement.
* **Lines and combos integrate.** 6224 at a 3x multiplier into a 6841 line, through the stock
  `session.rs` path with nothing special-cased.

No `Invalid endless flip install`, no `Missing native scorable`, no `Unsupported MotionGraph hook`,
no panics.

The one bail is ordinary skating, not a diagnosis — an octuple landed clean in the same session.
Whether the air budget ever starts a rung the skater cannot finish is still worth watching; the
headless wipeout that motivated the clip-derived budget is the shape to look for.

## Proving it stays retail-exact

The four pre-existing flip tests run unchanged **with the loop transitions compiled into the
graph**, and they are the guard: `held_flips_cycle_through_the_authored_double_triple_and_quad`,
`landed_quads_bank_and_name_the_quad_scorable`, `a_single_flip_is_the_ladder_baseline` and
`air_scoops_reach_the_authored_late_flips`. Every reward is identical to the pre-change baseline —
451.6692 kickflip quad, 563.5781 heelflip quad, 305.7779 single, 477.43457 / 477.59387 late flips.

```
SKATE3_ASSET_ROOT=C:/s3/installations/<install>/assets RUST_MIN_STACK=134217728 \
  cargo test -p skate-game --bin skate3rust flip_tests -- --ignored --nocapture
```

`RUST_MIN_STACK` is not optional; without it rustc dies with `STATUS_STACK_BUFFER_OVERRUN`
building the harness.

`endless_flips_cycle_past_the_authored_quad` replays the *same pop* with the feature off and on, so
the difference it asserts is the feature and not the boost.

> Note: `docs/flip-ladder.md` records the kickflip quad at 497 and the heelflip at 564, measured
> 2026-09-21. The heelflip still matches; the kickflip now measures 451.67 at the same boost. That
> drift predates this work and has not been investigated here.

## Known limits

* **Single-owner tuning.** Native trainer tuning allows one owning mod, so Endless Tricks and the
  Native Trainer cannot both be enabled.
* **No underflip past rung 4**, by construction — see the ordering note above.
* **`max_flips` above 16** has no named rung; the cap is enforced at 12 extra rungs in
  `TrainerTuning::valid`.
* **Replays** recorded with the mod on carry rungs a stock build has no names for.
* The extension rungs have **no authored audio**. `f.descriptor` also feeds
  `physics/audio_observation.rs`, whose trick table is keyed by `IDENTIFIERS`. The University
  session above raised no error from that path, so a minted name does fall through harmlessly;
  whether anything is audibly missing on an extension rung has not been listened for.
