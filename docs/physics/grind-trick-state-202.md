# State 202: grind trick exit

Crash report revision84142e78, tick64957: GrindDarkslide405 requested PhysicsAirSecondary202 with Processed2484 bit0x100000 set. The registry rejected it before native lifecycle publication; the first failure is a missing adapter, not a non-finite solver result.

## Original Skate 3 evidence

Read from the open TU3 IDA database default.patched.xex (SHA256431b8eba23565affdc10d137df19b06fe286244cefb3e1a13f32693e9600395a).

- PhysicalPlayer constructor82DB27AC allocates80bytes, installs vtable82327174 at82DB2808 and stores the owner at+1716 at82DB2824.
- GetType leaf82D33160 returns202 (li r3,0xCA). GetTypeString82D33168 returns82326F54, the string PHYSICS_STATE_GRIND_TRICK. PhysicsAirSecondary is the existing host enum name, not a second instance of the ordinary PhysicsAir200 class.
- Vtable slots: destructor82D33598, empty reset82B61BB8, Update82D336F8, empty FillPhysOut82B61BB8, Enter82D335F8, Exit82D33678, PredictFutureOfDeck82D34DA8, GetType82D33160, GetTypeString82D33168, empty slot36, PostPhysics82D33A70.
- Enter resets velocity48, counter64 and latch68; sets Skeleton16505; enables angular-only board animation (82C05658); selects wipeout mode1 and clears its balance only on a mode change.
- Update resets counter64 while Processed2468 bit0x4000 is set, otherwise increments it. After more than3 free updates latch68 stays set until Enter.
- It transforms Skeleton12048 trajectory translation times60 through Skeleton11920 animation-to-world axes into velocity48. The trajectory rotation also transforms the retained Reckoning1200 heading. Processed528 seeds Reckoning1136, then82D8E5C0/82D8C8F0 compute the ground-style reckoning. Skeleton82BDDA10 uses fast blending; Skeleton16388 is armed.
- Board update82D33928 zeros steering, selects active-mode wheel material, computes linear drag via82D94A20, and applies the normal collision response82D944E8 as force15 if present. Otherwise it sets the calculated drag.
- Post82D33A70 calls ground-animation wipeout checks82D8F918 with scale2 before release and0.5 after. Unless Processed2472 bit0x8000 is set, it projects the deck velocity onto normalized authored motion and blends that component toward authored motion with native constants0.9 and1.4 (82098F70/74); perpendicular velocity is preserved.
- Exit clears drag and restores standard wheel materials. No ordinary-air trajectory prediction/following is substituted.

## Host integration

A dedicated GrindTrick owner implements these methods. State202 is accepted by transition, pre-state and output registries, dispatched during the state update and post-physics phases, and retains the existing native selector's return to200 when the grind-trick flag clears. Existing darkslide Exit runs before202 Enter.

Regression coverage includes the reported405->202 transition,202 exits, four-frame sticky latch behavior, and authored-velocity decomposition. A stock-asset headless test executes405 Exit,202 Enter/Update/Predict/Fill/Post and202 Exit into200 without creating a game window.

Validation: cargo check succeeded; three focused tests passed; the ignored stock-asset lifecycle test was explicitly run with SKATE3_ASSET_ROOT=assets and passed. The exact gameplay sequence on the reported rail was not replayed.
