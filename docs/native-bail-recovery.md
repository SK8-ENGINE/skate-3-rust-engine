# Native bail checkpoint investigation

Status: **selector port only; gameplay integration is incomplete**. The existing
fixed-spawn response in `physics/teleport_state.rs` remains active. This change
must not be described as a working native bail-respawn fix.

## Reference and scope

Reference: TU3 memory image `default_82000000_011B0000.bin`, mapped at
`0x82000000`, SHA256
`f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4`.
Addresses below were inspected in the matching generated PPC instruction
comments; constants were checked against the raw image. The read-only IDA
lookup for `82BFB1E0` found no defined function in the locally available dump
database, so it did not provide independent decompiler confirmation.

No game, recomp, controller harness, screenshot capture or asset-check launch
was performed. No binary/data files belong in this commit.

## Implemented core

`skate-core/src/player/respawn.rs` contains the actor history and ordinary
automatic selector, with required scene-validation callbacks. It has no host
default that silently accepts missing geometry or occupant providers.

- `82BFB068` constructs the manager, seeds the history and starts a 20-update
  cooldown. The native 33-slot ring has 32 usable entries.
- `82BFB1E0` publishes current COM position every actor tick. Recording needs
  at least 120 empirical measurements, expired cooldown and a root at least
  1.5 units from every recorded position. Falling below 120 clears cooldown.
- `82BFB3F8` rejects State69. Riding states 100/104 publish the current
  orientation before testing state age, Ground323 and surface eligibility.
  Offboard states 500/502 require the state-age gate and no OffBoard333
  correction. Both paths require strictly more than the configured 15 frames.
- Surface categories 5, 6, 9, 12 and 13 are rejected for riding/ground checks.
  Category 8 requests offboard recovery. Scores are 100 for categories 1/2,
  50 for 3/4, 20 for 11, and zero otherwise.
- Recording checks location, edges, then ground unless alternate-world mode
  bypasses ground. It stores the original candidate, discarding the ray hit's
  position and offboard result.
- `82BFC9B0` tries current position (`82BFC038`), scored history (`82BFC378`),
  then fallback (`82BFC718`). Current selection checks ground, location,
  occupants and edges, in that order. An offboard ray result replaces only Y.
- `82BFC550` ranks newest to oldest: newest gets a 100-point penalty; entries
  older than five get a 200-point penalty. Strict comparisons make newest win
  ties. Selection removes the entry before validation, including successful
  entries. History validation repeats location/occupant checks only.
- Exhaustion falls back to initial transform, **current actor stance**, on-board
  mode and score zero. Manual/session-marker selector `82BFC828` has a different
  non-destructive history walk and must remain separate.

The six compile-only unit cases cover consumption/validation order, rejection
and reranking, current-position Y correction, cooldown/distance/capacity
boundaries, surface categories, and exhausted-history stance. They have not
been executed. This is a scalar Rust translation, not demonstrated bit-exact
PPC floating-point equivalence.

## Required host bindings

| Native input | Producer / current Rust connection |
| --- | --- |
| State20 | Conditioner164, incremented by `82DE5858`, reset by `82DE5588`; **not** frames in current physical state. No equivalent empirical counter is currently published. |
| State4 / State16 / State69 | Published physical state age, ID and teleport request. |
| Skeleton64 | Physical skeleton COM16144, published by `82BE1AE8`; corresponds to physical reckoning vector64. |
| Skeleton416 | Translation of Skeleton368, the unmirrored animation-to-world11920 matrix; corresponds to `animated_skeleton.roots.animation_to_world`. |
| Ground323 | `82DB6EC0` reads PhysicalPlayer1872 (GrindManager), word460 bit27. Constructor `82D8A318` places its 416-byte result at +48; the relevant result word is +412. The live Rust grind owner has no published equivalent. Its actual set/clear producer still needs recovery. Do not substitute `active`, `grounded`, or constant false. |
| OffBoard333 | Biped708, the offboard contact-correction active flag. |
| OffBoard52/56 | Both are populated from Processed2596 in `82DB6EC0`; category is packed bits7..11. Do not use Processed2600 for the second field. |

`82591E30` builds the recording transform. Riding reads the actor's board
frame through `82592A00` and adds 0.2 Y. Offboard uses Skeleton432 (mirrored
variant from `82BE3650`). If deck velocity squared exceeds 0.25, it normalizes
the velocity, constructs a world-up basis, and accepts that basis only when
the projected forward length squared exceeds 0.9. The exact host board-frame
binding and floating-point operation ordering still need completion.

`82BFBC18` traces from candidate position +0.1 Y with delta [0,-10,0], using
query `82E0AAD0` with actor identity. It checks normal Y, ray drop and category,
then tests the vertical capsule above the **ray start**. `82BFB6F8` queries
authored edges in a [0.3,0.6,0.3] half-bound, with the native provider order and
40-edge capacity, and rejects squared segment distance below 0.09. Triangle
diagonals are not a replacement for authored edge data.

`82BFB928` occupant predicates come from the LivingWorldManager provider
(SimController +212), not simply static collision geometry. `82BFBB48`
conditionally invokes the alternate-world location provider. The host adapter
must explicitly account for the actual available providers and world mode;
these predicates have not been connected or proven equivalent.

## Stock settings found

The installed collection data already includes class `Hash_12B64C0E804B0853`,
key `default`. These are read from data, not new tuning defaults:

| Field hash | Value | Use |
| --- | --- | --- |
| C0526C883AF0ECCA | 0.4 | Capsule height |
| CEB092E418A5B001 | 0.5 | Capsule radius |
| 8ABE098D3806D273 | 1.0 | Maximum ray drop |
| ADD032CACF6A1C15 | 0.75 | Minimum normal Y |
| E64C980EE2114070 | 1.0 | Occupant query radius |
| 59E1F2B3CDC4E658 | 2.0 | Occupant query radius |
| 10B7C3A9CC8D3721 | 15 (Int32) | Minimum state age |

Source `skatercollections.vlt` SHA256:
`3b7dbd062bb1c906a085514355afff35cfa22f486ae70820c5ad1a42a7aab25b`.

## Stance and deferred reset dependency

Actor reset `82592518` chooses the candidate, then calls `82592C08` to save
stance before queuing the reset reply (`825926F8`). Setter `82B97350` writes
animator15200; it does not immediately overwrite relative stance15196.
Getter `82592B68` combines natural15188, relative15196 and the fakie flag.

MotionGraph `ResetToGivenStance` invokes `82B97308` / `82B972A8`, consuming
saved15200, updating flags15180 and relative15196, then clearing saved15200.
The current game handler instead only clears the posture request. Saved15200
is absent from the canonical animation owner. The existing pure
`player/offboard/reset_animation.rs` helper and `motion_offboard/reset.rs`
owner contract are not integrated into this dispatch.

`ResetSkaterAnimation` also needs selective native owner resets: the existing
`MotionAnimation::reset_from_stock` replaces all animation flags with zero,
where `82B98050` applies a selective mask. Connecting saved checkpoint stance
without completing this lifecycle would not preserve native recovery.

Keep State702's request -> next actor input reply -> State702 output -> physical
reset ordering. Preserve the existing wipeout timers and recovery countdown.
Do not reset bodies from the selector. Map replacement should create a fresh
history; ordinary automatic recovery should retain the consumed history.

## Validation and remaining work

The Windows release game build passed with the requested static CRT flags.
After the fallback correction, the core library and six new cases compiled
with `cargo test --release --locked --target x86_64-pc-windows-msvc -p skate-core
--lib --no-run`. No test executable was run.

Remaining: recover the grind flag producer; complete canonical stance/reset
ownership; bind transforms and empirical counter; implement native scene
predicates; connect observation and actor callback at their publication
boundaries; compile the integrated executable. Gameplay parity remains for
user testing after those dependencies are completed.
