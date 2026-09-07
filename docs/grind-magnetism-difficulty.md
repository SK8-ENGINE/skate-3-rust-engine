# Difficulty-dependent grind magnetism

Observed in assets/private/stock/skater-collections.json, physics_mode:

| Mode | GrindLockDist bits | Metres |
| --- | --- | --- |
| easy | 3FACCCCD | 1.35 |
| normal | 3F666666 | 0.9 |
| hardcore | 3E19999A | 0.15 |

The field is native physics_mode+80. The live SelectorInput already receives
this field when the active difficulty changes. It does not currently result in
magnetic acquisition because AirTrajectoryRuntime passes WorldWithoutGrindEdges,
and KnownAir's StartGrindAirAdjust callback reports an unsupported owner.

Observed TU3 path: ScoreTrajectories82D68FE8 attempts82D69C00 on the first-pass
middle trajectory. Its actual spline query uses bounds around the natural landing
point: [-2,-.5,-2] to [2,4,2].82D6A840 retries ranked candidates and applies
physics_mode80 together with speed, ledge-side, tip and angle restrictions.
82D68C80 publishes9653 when the accepted middle trajectory wins, suppressing the
ordinary normal-delta second pass. Merely changing the score or allowing the
query is insufficient to connect the airborne adjustment owner.

KnownAir Enter82D352D0 passes the primitive endpoints and complete investigation
record to Start82D34FE8. The latter resets translation/rotation histories and
calls82D72B40 to build seven contact choices from stock deck/grind-air settings.
ProcessData82BDCFB0 calls82D712E0 and applies its pose result.82D712E0 owns the
projection82D71430, five-point crossing82D721A0, contact choice82D723D0,
translation82D72610 and rotation82D72810 stages. The stock correction limits
include MaxOffsetDist=.2m and MaxOffsetDeltaPerFrame=.03m; these are distinct
from the difficulty-dependent initial capture distance.

Native source identity: TU3 default_82000000_011B0000.bin SHA256
f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4.
Instruction extracts are in local logs/biped-*.txt. No gameplay tests or reference
process were run. Experimental selector plumbing is saved in
logs/magnetism-in-progress, outside the production source: the incomplete pose
owner must be connected before exposing9653 in the playable game.

Status: findings confirmed statically; magnetism is NOT fixed in the staged game.
