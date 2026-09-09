# Airborne board adjustment release (2026-09-09)

Releasing the right stick could leave the board held in its adjusted pose without
a grab. `ActionHost::end` only removed the generic `MGIntent`; `BoardAdjust`
instead publishes `MGIntentMag` and `MGIntentAngle`. A graph exit without another
Update left these values present, so the motion filters kept targeting the old
pose.

TU3 evidence: constructor `82BA2A30` stores the authored magnitude and angle
names at offsets `0x1C` and `0x34`. Vtable `8231E624` selects End `82BA2EB8`,
which gets the actor's motion-intent map and calls removal `82BC1068` for both
names (`82BA2F20`, `82BA2F2C`). The Rust End callback now performs those removals.

Stock `MotionGraphIncludes/BoardAdjusts/T_BoardAdjust.xml` already filters missing
inputs toward zero using `blendOut=0.08`, acceleration limit `0.03` and velocity
limit `0.2`, and exits to InAir when both absolute filtered values are below
`0.4` and the directional intent is absent. No filter coefficients changed.

The regression holds the adjustment, ends directly on release with no final
Update, and runs the stock filter settings. It covers all four output names,
mirrored angle signs, reentry, and preservation of unrelated motion intents.
Gameplay appearance remains for user testing; no game was launched.

Validation: the regression failed before the fix with retained values 0.9/0.8;
all six action-host tests passed after it (release, Windows MSVC, static CRT).
