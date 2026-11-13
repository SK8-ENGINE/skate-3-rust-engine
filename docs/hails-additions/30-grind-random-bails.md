# 30. Grinds: random bails at some rails (handrail bend fixed; corners open)

Branch `fix/grind-random-bails` (from `main` b3c9679). Status: **the handrail bend bail is fixed by porting
retail's per-volume world-query box (third pass, below).** The first two passes found the cause chain and
ruled out candidates 2 and 3; the third pass found the rule that differs from retail: the box every world
triangle is tested against before the pair query. The 90 degree corners (`0x656`, `0x694`) bail by design: two rails
meet there, and the skater has to trick over the corner (user decision, not a bug).

## Problem

The user reported that some grind spots make you bail at random, also on main, and that this happens for
sure on all three or four sets of rails at the PCU Library spawn
(University, PCU Library spawn). It happens on upstream `main` too.

## Evidence from the user's sessions

The audio state log (`state_*.tsv`, one row per 60 Hz step) of the session of 2026-10-03 11:53 has two grind
bails at the rails next to the PCU Library spawn (spawn locator `Z_UN_TrainingExterior`, (308.7, 74.0, -439.0)):

| Frame | Grind | Speed | Where | States |
|---|---|---|---|---|
| 15243 | boardslide (400), grind tag 16 | 8.2 m/s | bottom end of the stair handrail, (264.5, 73.1, -426.3) | 400 -> 500 -> 300 |
| 16008 | boardslide (400), grind tag 16 | 7.3 m/s | flat rail at its 90 degree corner, (263.1, 74.9, -430.1) | 400 -> 500 -> 300 |

400 is `GrindBoardslide`, 500 `BipedGround` (the grind state selector takes it when the run-out animation
attribute `OB_Traj` sets processed bit 2476.15), 300 `WipeoutGround`.

## Reproduction (headless)

`crates/skate-game/src/tests/grind_bail.rs` (ignored, needs the private assets and the converted University
map) lists the grind splines near the library stairs from the map's grind provider, drops the skater onto one
moving along it at 8 m/s and traces the physical state:

```
set SKATE3_ASSET_ROOT=<repo>\assets
set SKATE3_GRIND_MAP=<repo>\data\installations\<id>\maps\University.skate
set SKATE3_GRIND_RAIL=26  & set SKATE3_GRIND_REVERSE=1 & set SKATE3_GRIND_SLIDE=1 & set SKATE3_GRIND_LEAD=3
cargo test --locked -p skate-game --release --bin skate3rust -- --ignored --nocapture grind_bail_trace
```

| Spline (index in the listing, owner) | Approach | Result |
|---|---|---|
| 26, `0x688`: stair handrail, (275.7, 75.1) down to (264.2, 73.0), bends down into a post at the bottom | boardslide, downhill | grinds 53 ticks, wipeout at the bottom bend (tick 101) |
| 26, `0x688` | 50-50, downhill | grinds 53 ticks, 400 -> 500 (run-out) -> 300 at the bottom bend (tick 103): the user's sequence |
| 14, `0x656`: flat rail at y 74.91, ends in a 90 degree corner into spline 28 (`0x694`) | boardslide | wipeout at the corner (tick 105) |
| 10, `0x63c`: flat straight rail, free end | boardslide | grinds the whole run (104 ticks), no bail |
| 28, `0x694`: the flat rail after the corner, ends in a second 90 degree corner into spline 10 | boardslide | grinds 42 ticks, wipeout at that corner (tick 92) |
| 26, `0x688`, uphill | boardslide | grinds the whole run (104 ticks), no bail |
| stair edges (21 splines, 7 to 8 m each, `SKATE3_GRIND_LEAD=1.5`) | boardslide | 17 grind 85 to 93 ticks without a bail; spline 23 (`0x677`) bails after 49 ticks (not analysed); 4 never reach a grind (the drop misses the edge) |

So the bails are reproducible and not random: they happen where the rail bends (the handrail's bottom bend, a
corner). A straight rail does not bail.

## Cause chain (measured with temporary traces, removed again)

All three bails are wipeout request **2** from grind Post: the board's closing velocity is above the limit.

1. Grind Post `82D43098` [code] tail-calls `82D90898` [code] (ported in
   `crates/skate-core/src/physics/grind_forces/post.rs`). Its closing check `82D90AB8` [code] requests reason 2
   when the board is in contact (`+868`) and the horizontal part of the closing velocity `+656` in animation
   space is above `xz_acceleration_204` (6.0 [data], physics_wipeout) or its vertical part above 100.
   Compared instruction by instruction with the port: same.
2. The closing velocity `+656` is `normal * dot(normal, old part velocity)` of the board contact report with
   the largest closing speed (`82C07ED0..7F2C` [code], `board_ground.rs`). Only solved rows with a positive
   normal impulse are reported (`82AE1668..168C` [code], `board_reports.rs`).
3. Handrail bottom (spline 26), tick 99: the **deck** gets a contact with a 5 cm triangle of the post below the
   rail's bend, (264.33, 72.92, -426.17), face normal (0.97, -0.24, -0.02), flags `0xF71` (one-sided, edge
   cosines, edges 0 and 1 convex, all three vertices disabled), edge cosines (0.923, 0.9997, 0.961).
   - The deck is 17 cm above that triangle (`separation` 0.168), but the predictive limit along the triangle's
     face normal is 0.178 (`world_separation_limit`: approach `-n . v / 60` = 0.128, plus padding 0.05), so the
     pair is kept.
   - The raw contact normal is mostly vertical (-0.35, -0.92, 0.15). The triangle fixup `82AD3130` [code]
     classifies it as a disabled vertex and takes the TU3 (title update 3) recovery in `82AD2E00` [code] (one convex and one
     non-convex edge with cosine 0.961 <= 0.97): the normal is bent perpendicular to the convex edge, which is
     nearly vertical, so the bent normal is horizontal, (0.956, -0.258, -0.142) toward the deck. `82AD2BF0`
     [code] reprojects the contact points along it: 2 cm apart.
   - The solver applies an impulse, the row is reported, and the closing speed is 7.95 m/s (the whole slide
     speed): reason 2.
   - The vertex argument mapping (edges `+64/+96`, cosines, convex bits 0x20/0x80) and the recovery's edge and
     cosine choice were compared with the TU3 code: same as the port.
4. Flat rail corner (spline 14), tick 104: the deck reaches the cross tube of spline 28 at the corner. The deck
   rides with its centre at the spline height (the spline is the rail's top line; deck centre 74.92, spline
   74.915), so the cross tube's upper facets meet the deck front: unbent contacts at 6 cm with normals
   (0.83, 0.39, -0.39), closing speed 6.30 m/s > 6.0: reason 2.
5. One request of reason 2 with an upright animation is a run-out (`82DB9188` [code], `requests_runout`), so the
   50-50 goes to BipedGround first; BipedGround Post then requests 31 or 32 (stumbling, `ground_lifecycle/post.rs`)
   and the skater wipes out.

## What is not known yet (the actual root cause)

Every function inspected along the chain matches the TU3 code. Retail does not bail on these rails (the user),
so retail must differ before the chain starts. Candidates, in the order to check:

1. **Linked splines at corners.** The grind provider keeps the native link bounds (`grind_world` tests
   `native_payload_and_inclusive_link_bounds_survive_conversion`), but `grind_contact.rs` says "linked/adjacent
   primitive arbitration belongs to the caller". If retail hands the grind over to the linked spline (and turns
   the board) before the deck reaches the cross tube, the corner bail cannot happen in retail. Check the
   manager's primitive change at a link in the TU3 code (`82D89150` and its callers) and with a recomp trace of a
   boardslide around a corner.
2. **Board ride height during a grind.** If retail holds the deck a few centimetres higher (for example the
   board target is offset from the spline by the truck or deck height), both bails disappear: the corner tube
   is passed over, and the post triangle leaves the predictive range (0.168 against a limit of 0.178, a 1 cm
   margin). Compare the deck height over the spline in a recomp trace of a boardslide.
3. **Predictive contacts against grind rails.** Whether retail runs the deck's predictive world query against
   the rail mesh while grinding at all (collision group 4 is set by `grind_materials.rs`; the stock exclusion
   table `82767D60` only has (7,12) and (16,12)).

No threshold was changed: the 6.0 m/s limit and the 0.97 bend cosine are retail values, and a fudge would
hide real impacts.

## Second pass: static TU3 reads and contact reports (2026-10-08)

### Candidate 3 (rail mesh filtered for the board while grinding): not supported by the code

- The rail triangles are ordinary units of the same clustered mesh as the ground around them (University mesh
  `cSim_250_-450_high.xsf#0`, unit group 0, surface `0x90`: surface type 1, audio surface 16, the "grind tag 16"
  of the session log). Nothing in the data marks them as grind-only.
- The triangle dispatch `8277BC58` [code] skips a moving volume only by its byte `+216`, its kind 6, a (volume
  group x object group) bit table (job `+40`) and a (volume group x surface type) bit table (job `+44`; the
  stock initializer `82767D60` [code] sets only (7,12) and (16,12)). With one mesh and unit group 0 there is no
  rail-specific entry.
- The board's group is 4 both while grinding and outside: the grind material switch `82D89DC0` [code] (taken
  when 2512 is 400 or 2508 is 701) and the standard switch `82C090C0` [code] both write 4 to the assembly `+24`
  and to every part's `+92`; they differ only in the materials (`+8316/+8328/+8340` against
  `+8280/+8292/+8304`).
- Grind Enter `82D3F318` [code] (animation drive off, `16505`, feet volumes `82D91330`, deck angular drag),
  Exit `82D3F430` [code] (feet volumes, `82C091F8`, drive off, wheel spin `82D3F7D0`) and the Nonspecific
  Enter `82D42D80` [code] change no board volume enable and no query setting.
- The closing-velocity loop of `82C07D20` (`82C07E68..82C07F30` [code]) runs over every report of all seven
  parts, wheels included, with the old part velocity: the port's `board_ground.rs` matches.

So, statically, retail queries the rail mesh with all board parts while grinding, as the port does.

### Candidate 2 (ride height offset): ruled out as an offset

The boardslide leaves are forces plus an angular-only hook drive (`82C05658` [code]); none holds a height. The
deck rests on the tube: tube top 74.914 (the spline height), deck centre 74.92 = tube top + the deck's half
thickness (rounded box half extent 0.00075 + radius 0.00675). The ride height is a contact result, so a retail
difference there would have to come from the contacts, not from an authored offset.

### What the board actually hits (headless, per-tick board contact reports)

Corner, spline 14 (`0x656`), boardslide 8 m/s, default lead 1.5 m (grind 70 ticks, wipeout tick 118):

| Tick | Reports |
|---|---|
| up to 115 | deck only, normal (0, 1, 0) on the tube top (y 74.914) |
| 116 | **left front wheel**, normal (0.91, -0.04, -0.42) at (262.891, 74.867, -430.370): the leading wheel meets the side of the cross tube (spline 28, `0x694`) below its top; the deck centre is still 0.17 m before the corner and the deck edge about 5 cm before the cross spline |
| 117 | BipedGround (run-out); deck now also against the cross tube's upper facets (0.6, 0.75, -0.27) |
| 118 | WipeoutGround |

So with this approach the first impact is a real wheel collision, not a predictive ghost: in a boardslide the
wheels hang below the rail top on both sides, and at a 90 degree corner the cross tube lies across the
leading wheels' path. (The first pass, from another start position, saw the deck reach the cross tube first; either
way the board runs into the continuing geometry at the end of the spline.)

Handrail, spline 26 (`0x688`), downhill, lead 3, boardslide (wipeout tick 101): ticks 96 and 97 the front
wheels touch the rail side (normal (-0.45, 0.38, -0.81)) while the deck rides the top; tick 98 the deck reaches
the rail end where it bends into the post (deck report (-0.17, -0.29, -0.94)); tick 99 the post triangle
report (0.96, -0.26, -0.14) (the vertex-recovery normal of the first pass) and the deck's speed drops from 8.2
to 0.9 m/s in one tick; 100 BipedGround; 101 WipeoutGround.

### Conclusion of the second pass

Both bails start when the board reaches the **end of the grind spline where the geometry continues** (a cross
tube, a post). Every function checked along the physics chain matches TU3, and candidates 2 and 3 are not
supported by the code. That leaves candidate 1: what retail's grind does at the end of a spline. The admission
`82D886B8` [code] allows StayInGrind up to 90 degrees, so a perpendicular linked primitive can be taken over,
but in the port the leading wheels reach the cross tube before the deck's contact query reaches the linked
spline. Whether retail hands over (and turns the board) earlier, ends the grind before the end, or lifts the
board, was not found in this pass: the manager `82D8A828` [code], the investigator `82D875A8` [code] and
`82D89150` [code] were not re-read instruction by instruction against the port at the spline end. That read,
or a recomp trace, is the next step.

Two points that bound candidate 1 at the corner:

- The boardslide deck contact (`82D889A8` [code] admission, port `boardslide_candidate` and the deck rectangle
  query before it) intersects the grind segments with a vertical rectangle along the deck's long axis through
  its centre. In a boardslide the deck lies across the rail, so the cross spline of a 90 degree corner runs
  parallel to that rectangle and is never found: the port cannot hand over at such a corner, and the same
  query in retail cannot either. A retail hand-over would have to come from another path.
- The leading wheels hang about 5 cm below the deck (wheel centre 74.867 against the tube top 74.914), so at a
  90 degree corner of a rail at one height they cross the cross tube's line geometrically. Whether retail's
  board really slides through that corner, or also stops there, should be asked: the user's report names the
  PCU Library rails as bailing in this engine; that retail boardslides through this particular corner is not
  confirmed yet.

The handrail case is different: the deck slides off the rail end that bends down into the post, and the
contact that stops it is with a post triangle 17 cm below the deck (a predictive contact with the vertex
recovery normal of the first pass). That looks like the stronger case of an engine-side difference.

### Recomp trace plan (not run)

Hooks (research-hooks, guarded reads, category `grind`), logged only while 2508 is 400..405 or 701, plus the
two ticks after leaving:

1. `GRMGR` at the end of the manager pre-update `82D8A828`: 2508, 2512, selected owner `+1296`, primitive start
   and end `+1264/+1280`, point `+1120`, family `+1248`, entry kind `+1252`, flags `+1516`, deck frame.
2. `GRCAND` at entry and exit of `82D89150`: r3 to r8, the two truck contact primitives and the return value.
3. `BRDREP` at `82C07ED0` in the `82C07D20` report loop: part index (`+64`), normal, position and surface of
   every report.
4. `BRDPOSE` once per tick: the seven board part positions (the wheel heights against the tube top).
5. `GRPOST` at `82D90AB8`: closing velocity `+656`, its animation-space xz length and the request.

Action: following the standing rules, gameplay data comes from the user's play, not scripted skating. Verify in
one background run that the hooks fire, then ask the user for one short session: boardslide the flat PCU
Library rail into its 90 degree corner and the stair handrail off its bottom bend, three times each. Compare per
tick with the headless traces above: does retail switch owner `0x656` to `0x694` (or leave 400) before the
corner, where are the wheels relative to the tube top, and does any wheel or deck report exceed 6 m/s.

## Third pass: the per-volume query box (fix, 2026-10-08)

### Retail (TU3, instruction level)

- `82777E70` builds one box per volume before the triangle dispatch (stored at context +12, 32 bytes each).
  In my own words:
  - It calls the volume's bounds method (vtable at volume +64, slot +4, with the volume transform). For a
    triangle volume that method is `82ADDC40` (TriangleVolume::GetBBox): vertex min/max minus/plus the shape
    fatness (volume +80). The flag argument is not read there, and the +112 padding (volume base +112) is not
    used. So the bounds are the shape plus its radius/fatness, nothing more.
  - Linear step = v * dt + clamp(dot(v * dt, a * dt^2), 0, 1) * a * dt^2, from body +32 (velocity) and +144
    (force acceleration); dt is the data constant at 0x820849C8 (1/60).
  - Angular step, same form, from body +48 and +160. Rotation pad = max(|ex - ey|, |ey - ez|, |ez - ex|) of the
    bounds extents, times min(|angular step|, 1).
  - Box = (min - pad, max + pad) unioned with itself moved by the linear step, then scaled about its centre by
    the data constant at 0x821659FC (1.05).
  - There is no maximum-separation (0.5) or padding (0.05) term.
- `8277BC58` BE94..BF38 then tests, for every (triangle, volume) pair, the min/max of the triangle's three
  vertices (no fatness) against that box; a pair disjoint on any axis is skipped before `8277B720` runs.
- The port used `conservative_bounds(shape bounds, 0.05 + 0.5 + maximum fatness)` for this test, and only when
  the world had query metadata: up to 55 cm more on every axis, in every direction.

### Headless check before porting

Rail `0x688`, boardslide downhill, lead 3, the deck contacts around the bend (ticks 96 to 99): every contact
with the post triangles below the rail end failed the retail box test. Example: deck boxes y 72.97..73.21, post
triangle vertex boxes y 72.47..72.94. The stopping contact (17 cm below the deck) is therefore never queried
in retail; the port admitted it only through the 0.55 m pad.

### Change

- `skate-core/src/physics/board_world/broadphase.rs`: `volume_query_bounds` (port of `82777E70`),
  `VolumeMotion` (the four body rates the box sweeps over), `VOLUME_QUERY_STEP` / `VOLUME_QUERY_SCALE` (the two
  data constants).
- `BoardWorldVolume` carries `motion: VolumeMotion` instead of only `linear_velocity`; every builder (board
  colliders, skater skeleton, network/attached bodies, the sphere convenience query) fills it from the body
  rates.
- `BoardWorld::query_primitives`: the per-triangle box test uses the retail box and runs for every triangle,
  as in `8277BC58`. The BVH cluster preselection stays conservative (union of the old padded box and the retail
  box), so it can only add candidates.

### Results (headless, 8 m/s, same test, before -> after)

| Run | Before | After |
|---|---|---|
| `0x688` boardslide downhill, lead 3 | 53 grind ticks, wipeout tick 101 | 55 grind ticks, grinds off the end, lands, rolls (no wipeout in 260 ticks) |
| `0x688` 50-50 downhill, lead 3 | wipeout tick 103 | 50-50 then 5-0 to the end, lands, rolls (no wipeout) |
| `0x688` boardslide uphill (index 26, lead 1.5) | 214 grind ticks, no bail | 126 grind ticks, no bail |
| `0x63c` straight (index 10) | 166 grind ticks, wipeout 213 (after the grind) | 164 grind ticks, wipeout 214 |
| `0x656` corner (index 14) | 70 grind ticks, wipeout 118 | 70 grind ticks, wipeout 121 |
| `0x694` corner (index 28) | 42 grind ticks, wipeout 92 | 42 grind ticks, wipeout 92 |
| `0x677` stair edge (index 23) | 49 grind ticks, wipeout 137 | 82 grind ticks, no wipeout |

All 29 splines near the library, boardslide, lead 1.5 (index: grind ticks / wipeout tick, before -> after):
0: 89/0 -> 0/66, 2: 97/189 -> 0/0, 7: 86/0 -> 0/0, 11: 89/0 -> 0/0, 12: 93/0 -> 0/0, 15: 87/0 -> 0/0,
20: 87/0 -> 0/69, 25: 0/0 -> 93/0, 9: 213/0 -> 119/0; the others within a few ticks.
Most stair-edge starts do not land on the edge: the skater drops down the stairs and the grind (if any) comes
later, so these runs diverge at the landing, not at a rail.

### The stair landing speed (main open risk)

Index 2, landing at ticks 60 to 62: before, the board lost speed on landing (horizontal 7.5 -> 4.1 m/s by
tick 64); after, it keeps about 7.1 m/s and rolls on, so it never reaches the later grind. With temporary
prints (removed again) the contacts the retail box now culls on those ticks were listed, and two diagnostic
runs re-admitted one class each:

- culled contacts with a ground-like normal (y > 0.8; the ground 15 to 17 cm below the falling deck at tick 61):
  same outcome as after (6.8 m/s at ticks 62 to 64, rolls on);
- culled contacts with a wall-like normal (y <= 0.8; mainly a side face with normal (-0.42, 0.16, -0.90),
  1 to 4 cm from the deck, the board moving away from it at about 3 m/s): the speed loss comes back
  (4.5 -> 3.4 m/s by tick 66) and that run even wipes out.

So the old landing slowdown came from predictive contacts with a nearby side face the board was moving away
from. The retail box sweeps only along the motion, so retail does not query that face either; by the ported
rule the new landing is the retail one. Not measured in retail: a recomp stair landing (deck speed per tick)
would settle it.

### Test changed: `approaching_velocity_padding_and_world_fatness_control_real_contact_acceptance`

Old assertions (sphere 0.01 above a plane triangle, `maximum_separating_distance` 0.004):

- moving up at 1 m/s with `volume_padding` 0.02: 4 contacts;
- moving up at 1 m/s, padding 0, triangle fatness 0.02: 4 contacts, contact point at y 0.02.

Both scenarios are culled by retail before the pair query: the triangle's vertex box is y 0 (no fatness,
`8277BC58` BE94..BF38) and the volume box starts at y 0.01 and sweeps only upward (`82777E70`; the bounds
method adds the shape fatness but not the +112 padding, see `82ADDC40`). New assertions: the same padding and
fatness cases while approaching at 1 m/s (the 0.004 limit alone is still too small, so padding / fatness are
what admit the pair): 4 contacts each, fatness point at y 0.02; and the rising case with padding 0.02 now
asserts 0 contacts. The production world triangles have fatness 0 (`skate-game/src/physics/ground.rs`), so
the fat-triangle case is synthetic. Evidence strength: the triangle box read is direct (vertex loads, compare,
skip); the "no +112 padding" read rests on `82ADDC40` only; the bounds methods of the sphere, capsule and box
volumes were not read and are assumed to follow the same pattern (shape plus radius). (Fourth pass: now read,
see below.)

## Fourth pass: shape bounds, rotation pad arithmetic, stair landing (2026-10-08)

### Retail (TU3, instruction level)

- **Shape descriptors.** Volume +64 points at a per-shape table: word 0 is the shape kind, +4 the bounds
  method that `82777E70` calls with flag 1. The tables sit together in the image at `0x82FD57A8..0x82FD5848`:
  sphere (kind 1) `82ADD738`, capsule (kind 2) `82AD97A0`, triangle (kind 3) `82ADDC40`, box (kind 4)
  `82AD9558`, and kind 5 `82ADA578` (not used by the board or skater). The kinds match the port's projection
  callbacks (sphere `82ADD800`, capsule `82AD99C8`, box `82AD8508` sit next to their bounds methods).
- **All four bounds methods ignore the flag and add only the radius / fatness at +80.** None reads the +112
  padding or any separation term. With a transform (r4) they first move the shape into it; the port's
  primitives are already in world space.
  - Sphere: centre (+48) minus / plus radius (+80).
  - Capsule: per world axis, extent = fma(|axis (+32)|, half length (+68), radius (+80)); bounds = centre
    (+48) minus / plus extent.
  - Box: per world axis, |axis 1 (+16)| * h (+72), then fma |axis 0 (+0)| * h (+68), then fma |axis 2 (+32)| *
    h (+76), then + radius (+80) as a separate add; bounds = centre (+48) minus / plus that.
  - Triangle: min / max of the three vertices, then minus / plus fatness (+80) (third pass).
- **Rotation pad length.** `82777E70` takes the angular step's length as: x = vmsum3fp128(step, step); y =
  vrsqrtefp128(x); two refinements y = fma(y * 0.5, fnma(x, y * y, 1), y); length = x * y, selected to 0 when
  x == 0 (vcmpeqfp + vsel); then min(length, 1). It is the same sequence as the port's
  `board_motion_output::length`.
- The rest of `82777E70` was re-checked against the port: the step terms (rate * dt fused with
  ((acceleration * factor) * dt) * dt, factor = min(max(dot(rate * dt, (acceleration * dt) * dt), 0), 1)), the
  extent-difference maximum, the pad, the swept union and the 1.05 scale about the centre (centre = (max + min)
  * 0.5, half = (max - centre) * 1.05) are in the same operation order.

### Change

- `primitive_bounds` (`board_world/broadphase.rs`): capsule and rounded box now compute their extent in
  retail's operation order (the fused multiply-adds above). Same values to float precision as before; only
  the last bit can differ. Sphere and triangle were already the same.
- `volume_query_bounds`: the rotation pad length uses `board_motion_output::length` (refined reciprocal
  square root) instead of `sqrt`.
- New skate-core tests: `volume_query_rotation_pad_uses_refined_reciprocal_square_root` (a 3.09 rad/s spin:
  the refined length is `0x3D52F1AB`, host `sqrt` gives `0x3D52F1AA`; the test checks the box bits use the
  refined one) and `volume_query_shape_bounds_follow_the_retail_bounds_slots` (capsule and box bits).
- Bit identity limit: the estimate itself (`vrsqrtefp128`) comes from `native_arithmetic`, which uses the
  host 1/sqrt, not the Xenon estimate table (project rule for all callers). With two refinements the result
  equals the console's whenever the two estimates refine to the same value; this was not checked against the
  hardware table.

### Results (before -> after this pass)

All identical: the 29-spline sweep near the library (boardslide, lead 1.5; per-tick logs at 2 decimals
identical for every spline), `0x688` boardslide and 50-50 downhill (lead 3: 55 / 57 grind ticks, no
wipeout), 47 data-gated physics / scoring / water drop / handrail tests (same pass / fail set, same output;
water drop mean 0.079 m/s), skate-core and skate-game unit suites (see Verification).

### The stair landing side face (static read with numbers)

Spline index 2 (lead 1.5, boardslide), landing ticks 58 to 68, with a temporary print (removed again) of
every pair the box culls although the pair query would return a contact. The side face from the third pass
is triangle 637472, normal (-0.417, 0.156, -0.895): a 3.4 cm high strip (vertex box y 72.420 to 72.454,
x 245.17 to 246.07, z -430.66 to -430.23), so a stair nosing / riser face. The board volumes (group 4)
overlap it in x and z on every tick; only y separates them. Gap from the face's top to the bottom of the
board volume's box (which already includes the downward 1/60 sweep and the 1.05 scale):

| Tick | 58 | 59 | 60 | 61 | 62 | 63 | 64 | 65 | 66 | 67 | 68 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Smallest y gap (cm) | 52.9 | 39.4 | 26.1 | 9.4 | 3.4 | 8.4 | 11.5 | 15.8 | 16.3 | 14.7 | 14.2 |

So retail's box, by the rule now ported for every shape, does not admit this face on any landing tick: the
closest miss is 3.4 cm at tick 62. It would be admitted only if retail's board were at least 3.4 cm lower on
that tick (or its downward velocity at least about 2 m/s larger, since one 1/60 step of sweep is
v / 60), i.e. a different pose, not a different rule. The old slowdown came from the 0.55 m conservative
pad, which reached the strip. Not measured in the recomp.

## Change

- New diagnostic test `crates/skate-game/src/tests/grind_bail.rs` (registered in `crates/skate-game/src/physics.rs`
  as `grind_bail_tests`), ignored and data-gated like `water_drop`. It lists the splines near a point
  (`SKATE3_GRIND_NEAR`), runs one approach per spline, prints the state per tick and the first wipeout tick,
  and dumps the collision triangles around a point (`SKATE3_GRIND_TRIS=x,y,z,r`) with their edge flags.
- Third pass: the retail per-volume query box (above). New tests: `volume_query_bounds_follow_82777e70`
  (skate-core, formula cases) and `library_handrail_bend_grinds_through` (skate-game, ignored, private data:
  `0x688` boardslide and 50-50 downhill and boardslide uphill each grind 50+ ticks without a wipeout).
- Fourth pass: capsule / box bounds in retail operation order and the refined reciprocal square root for the
  rotation pad (`board_world/broadphase.rs`); tests `volume_query_rotation_pad_uses_refined_reciprocal_square_root`
  and `volume_query_shape_bounds_follow_the_retail_bounds_slots` (skate-core).

## Verification

- The test reproduces the user's two locations (and a third approach, the 50-50) and shows a straight rail
  without a bail (table above).
- Existing tests, same before and after (no gameplay code changed): `cargo test -p skate-game --release --bin
  skate3rust -- grind` 52 passed, 4 ignored; `cargo test -p skate-core --release -- grind` 60 passed.
- The repro runs again without the temporary traces: handrail boardslide wipeout tick 101 (54 grind ticks),
  handrail 50-50 tick 103, corner tick 105, straight rail no bail.

- Second pass: no code changed. The contact reports and the RWCM unit groups were read with temporary
  prints in the diagnostic test (removed again); the prints did not change the outcome (corner, lead 1.5:
  wipeout tick 118 with and without them; handrail boardslide, lead 3: 101 as in the first pass).

- Third pass (box port), same target dir, before -> after:
  - skate-core full suite: 2 known failures on main (`predictive_contacts_and_retention_match_full_scan_for_every_primitive`,
    `a_moving_group_8_body...`) -> 793 passed, 1 failed (`a_moving_group_8_body...`, known). The
    predictive/full-scan test now passes: the per-volume test is the same with and without query metadata.
  - skate-game `--bin skate3rust`: 447 passed, 1 failed (`setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`, known).
  - Ignored data tests, before and after identical: physics air / powerslide / recorded / startup / wipeout
    (4 pass, 8 fail in both; the failures are data/environment), climbing (3 fail in both), offboard jump
    playbacks and scoring runtime pass. Water drop (University): settled part speed mean 0.080 -> 0.079 m/s.
  - `library_handrail_bend_grinds_through` passes after; before, the same runs wipe out (tick 101 / 103).

- Fourth pass (shape bounds order, refined rsqrt), same target dir, before -> after:
  - skate-core full suite: 793 passed + 1 known failure (`a_moving_group_8_body...`) -> 795 passed (2 new)
    + the same known failure.
  - skate-game `--bin skate3rust`: 447 passed, 1 known failure, both before and after.
  - `grind_bail_trace` (29 splines, boardslide, lead 1.5; `0x688` boardslide and 50-50, lead 3): identical
    per-tick logs. 47 ignored data tests (`physics::air_tests`, `climbing`, `offboard*`, `powerslide`,
    `recorded`, `water_drop`, `wipeout*`, `scoring_runtime`, `library_handrail_bend_grinds_through`): 38 pass,
    9 fail (data / environment, same set) with identical output before and after.

## Open questions

- Stair landing speed (third pass): the port now keeps about 7 m/s where it used to drop to about 4 m/s; by
  the ported rule this is retail, but it is not measured in the recomp. Watch landings next to walls/stair
  sides in play.
- Bounds methods of the sphere, capsule and box volumes: read in the fourth pass (shape plus radius +80, no
  padding); closed.
- Rotation pad: the refined reciprocal square root is ported (fourth pass). Open: the `vrsqrtefp128` estimate
  is the host 1/sqrt (project-wide), so the last bit can still differ from the console in rare cases.
- Stair landing (fourth pass): the side face misses the retail box by 3.4 cm or more on every landing tick,
  so the speed kept on landing follows from the rule; whether retail's board pose on those ticks is the same
  is not measured.

- Which rule retail applies at the spline end (second pass: candidates 2 and 3 are not supported by the static
  code; candidate 1 remains). Needs the instruction-level read of `82D8A828` / `82D875A8` at a spline end or
  the recomp trace planned above (the recomp is reference, not a console measurement).
- Handrail bottom: fixed by the box port; a recomp BRDREP trace would still confirm retail has no deck/post
  contact there.
- Stair-edge spline 23 (`0x677`) no longer bails after the box port (82 grind ticks, no wipeout); its old bail
  was not analysed.

## Moddability

The box port takes its step and scale as arguments (`volume_query_bounds(primitive, motion, step, scale)`),
with the retail data values as the named defaults `VOLUME_QUERY_STEP` / `VOLUME_QUERY_SCALE`, and it reads the
same body rates a mod-driven body already has, so modded bodies and volumes get the same culling without extra
work. Exposing the two values through the physics settings / Lua SDK is not done (no setup-data source for
them yet); open for the moddability pass. Corner rules, when found, come from the setup data as retail
defaults, like the existing grind settings.
