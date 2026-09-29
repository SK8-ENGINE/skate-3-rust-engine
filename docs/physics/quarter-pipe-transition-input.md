# Quarter-pipe transition input

Verified against both open IDA databases on 2026-09-13.

## Skate 3 TU3 evidence

Database: default.patched.xex, SHA256 431b8eba23565affdc10d137df19b06fe286244cefb3e1a13f32693e9600395a.

- PlayerInput phase 0x82DB4330 calls 0x825903C8 when actor input is available and animation suppression is clear.
- At 0x82590414, that helper loads the expression index from the action table at +0x104 (65 * 4), then evaluates it via 0x8296C350 at 0x82590420.
- The caller clamps the result to [-1, 1] at 0x82DB4334..434C and stores it into ProcessedPhysIn+0xA4C (2636) at 0x82DB4354.
- Vert adjustment 0x82D67D08 reads this field at 0x82D67F5C. The following arithmetic subtracts 0.25 and clamps to [-1, 1].
- At 0x82D68094 it forms world-up minus ground-normal times the clamped lean times VertJumpAlignMaxAngle. The steep-surface/upward-motion gates and alignment blend then determine the starting velocity; original speed is restored after direction normalization.

## Skate 2 corroboration

Database: sk82_na_m.xex, SHA256 3706cedabd24fcd3dd16c99405ff5725deab5b09739ae926324db88a0b7c0fe2.

0x82DBFBC0 is named Sk8::Physics::TrajectorySelector::AdjustStartingVelOnVerts. At 0x82DBFE10..FE1C the decompiler identifies the source as mProcessedPhysIn->mTransitionInput and shows the same subtract-0.25/clamp operation. Skate 2 corroborates the field and behavior; Skate 3 assembly determines the actual binding.

## Adapter defect and correction

The host previously supplied actions.value(71), the right trigger. GameplayActions maps action 65 to pad[18] - pad[19], the signed left-stick Y axis; action 71 maps to pad[11], the right trigger. Xbox conversion makes pushing the left stick forward positive.

Consequently, forward stick with no trigger supplied zero transition input (lean -0.25), whereas full forward should supply approximately +1 (lean +0.75). This reverses the requested lean relative to the inward ramp normal and explains the reported return into the quarter pipe. The host now supplies action 65, matching the original helper. No trajectory settings or forces were retuned.

## Validation and limits

Two headless regression tests pass: raw controller forward/backward/neutral/partial/deadzone input reaches the production adapter, and neither grab trigger nor right-stick trick input changes it. The dev executable compiled successfully. Gameplay was not launched or visually verified. Staging to bin/skate3rust.exe was blocked because that executable was in use.
