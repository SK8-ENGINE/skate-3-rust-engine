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
