# Runout crash investigation

The Hardcore 360-flip reports at ticks 583 and 378 terminate at MotionGraph
behavior 86, `AddRunoutAttribs`. This operation was unsupported before the
difficulty-menu changes. Entering the stock runout graph now succeeds in Easy,
Normal and Hardcore. A full-frame regression then exposed the next missing
owner: `PhysicsGround -> BipedGround` has no production adapter yet.

## Reference

TU3 dump `default_82000000_011B0000.bin`, guest base 0x82000000,
SHA256 f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4.
Registration 0x82F89700 selects factory 0x82BC9BA8; vtable 0x82320BDC has
Begin 0x82BBA4A8, Update 0x82BBA770 and empty End 0x82B61BB8.
The initializer pair 0x82F85870/0x82F85888 identifies BipedStartAngle/BipedSpeed.
Addresses are guest addresses; subtract 0x82000000 for dump offsets.

Observed: Begin reads Skeleton0 (effective animation root Z),
SystemReckoning16 (COM velocity), SystemReckoning96 (up), and mirrors through
ISkaterAnim slot28. OffBoard331 selects alternate velocity OffBoard128.
It uses 0x8286CD88 for the projected signed angle, wraps with the strict
fraction > 0.5 condition, converts radians to degrees using word 0x42652EE1,
and captures full three-dimensional speed. Update emits the retained values
with normalized=false and sequence=-1. End performs no writes.

The game currently publishes the ordinary velocity branch; existing riding/air
states leave OffBoard331 at its native reset value. The alternate branch is
implemented and tested but requires the future BipedAir output owner.

The host numerical routines follow recovered arithmetic; no hardware-matched
360-flip replay has been recorded. A test-injected runout request verifies the
stock animation branch in all three modes, not complete on-foot physics.

## Remaining integration

User explicitly selected implementing native on-foot/runout instead of an
error-recovery screen. BipedGround requires the contact toolkit (82D811C8 /
82D81610), ground job producer, controller placement/update, skeleton mode4,
board manager, state publications, and BipedAir/remount transitions. Existing
recovered modules are being compiled and connected; enabling modules alone is
not a working BipedGround implementation.

## Contact toolkit progress

The compiled offboard modules now include the existing native ground lifecycle,
sync, reckoning, board possession, reset animation, stock settings loader and
Skeleton mode4 adapter. The retained board transform has a distinct owner, as
required by the recovered BipedGround skeleton update.

`contact_queries` reconstructs82D856C8's three support probes and44 obstacle
queries, including reverse rays and the constructor's accumulated float rounding.
All41 probe definitions are checked word-for-word against an independently
evaluated constructor using the dump above. The game adapter queries actual map
triangles through the existing acceleration tree and retains query observations.
Nonidentity mesh transforms use the complete geometry path to avoid incorrect
world/local-space culling. Authored-floor and empty-space query tests pass.

`contact_records` ports82D81F80's projection, normal fallback, record routing,
flags, distance selection and64-record surface capacity. `contact_packet` ports
82D2DDD0's reset and82D81610..82D818A0's primary/side-support consumption.
Boundary tests cover the strict side-height limits, central-contact requirement,
world-Y normal replacement, deep-support flags, lateral-normal fallback and
record capacity. These tests validate recovered host arithmetic, not bit-exact
Xenon arithmetic or a hardware replay.

Still required: scene segment gathering inside82D81610; classifiers82D82498,
82D82940,82D830A8,82D83438,82D837C8 and82D84060 and their helpers; query-result
136 identity mapping; the production ground-job producer and lifecycle owners;
BipedAir and board/remount integration. These preparatory modules deliberately
do not enable BipedGround while its native contact producer remains incomplete.
The production runout transition still fails and the playable binary has not
been replaced with an incomplete on-foot implementation.
