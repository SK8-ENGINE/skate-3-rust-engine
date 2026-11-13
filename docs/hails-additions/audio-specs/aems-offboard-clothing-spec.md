# Off-board footsteps, clothing and hands-on-deck: retail spec (2026-10-02)

Retail's `SFXObj_OffBoard` (controller `0x40010090`), `SFXObj_Clothing` (`0x40010060`) and the
Contacts owner's hand-on-deck sounds, read from the TU3 (title update 3) recompilation (`tools/recomp-code-search/fn.sh`;
local asm dumps, local tools `keys.py` (resolve the
lis/ori vault keys of a dump against a collection) and `surfaces.py` (per-material footstep fields)).
Port: `crates/skate-audio/src/player/{footsteps,clothing,step_on}.rs` (work copy until integrated).
Reference only: upstream PR #4 / #1, the PoC, skate3recomp code. Constants/offsets/addresses are facts.

Vault holders used below (audio table `*(0x830CFDA4)`, built by `sub_8289D5C8`):
`+92` = OffBoard tuning (class `C1831BDB6CB1B1EA` collection `1ABD2984D7248589`), `+136` = Clothing
(`A867FBE3454326FF` `default`), `+140` = eEQChain holder (`42AFE160E647167C` `default`), `+112` =
Contacts foley (`923CCB46EF5BF5BA` / `E0C3B44AB44F7B90`), `+36` = class `6EBA5BCD3E38A98A` `default`,
`+64` = AudioSurfaceMap (`C1831BDB6CB1B1EA` / `C489459A0C07D154`, field `4CA607558B1CF440`),
`+236` = `physics_skeleton` (`9F862C2EAA016CE5` `default`). Key `D7EDBD362D7D2152` = the `default`
collection.

## 1. Owner identification
- OffBoard vtable `0x822FCF98`: create `sub_824E8F98`, process `sub_824E9270` (+36), update
  `sub_824E9628` (+40); factory `sub_824E8F08` names it from `0x8224A700` "SFXObj_OffBoard".
- Clothing vtable `0x822FCD10`: process `sub_824DBB68`, update `sub_824DCB98`.
- **Caller `824D8164` is not the player**: it is inside `sub_824D8078`, the process of vtable
  `0x822FCCC8`, factory `sub_824D7E00` → "SFXObj_PedestrianSFX" (`0x8224A414`). Retail's
  `sk8_foley` containers 62/63/64 by that caller (1017 events) are the pedestrians' footsteps. The
  player's containers 62/63/64 come from caller `824E95BC` (= `sub_824E9D10`, the walking voices).
- Clothing's own Splice sounds are `sk8_foley` 73/74 (caller `824DBB94` = `sub_824DBBB8`).

## 2. OffBoard process `sub_824E9270` (per frame, before the MixMap tick)
Gate: `[owner+28]` record present and its `+52` active. Then:
1. OffBoard.in0 = 32767 if state `+716` (on foot) else 0 (vfunc 8, id 0).
2. Curve words (`sub_82481E10(16, x = rec+16, y = rec+80, v)`: v < x0 → y0; v ≥ x15 → y15; else
   the first i with v < x[i]: dx = x[i] − x[i−1] > 0 ? (y[i] − y[i−1]) / dx × (v − x[i−1]) + y[i−1]
   (fmadds) : y[i]; stored with `fctiwz`):
   `+408` = walk(`+212` |COM v|): curve `C3CD069BB1B16B58`; `+416` = xz(`+284`), `+412` = xz(`+288`):
   `CF844597AB96EAF8`; `+424` = vertical(`+292`), `+420` = vertical(`+296`): `236311604A3C1FB5`.
   (Points in `FootstepTuning::default`: walk 0 → 0, 2.0 → 205, 5.13 → 355, 6.03 → 409, ≥ 7.35 → ~512;
   xz ≈ 512..529 above 0.3 m/s; vertical 2 below 0.5 m/s, 0 from 1 to 9.3, 3.5..8.3 above 9.45.)
3. Copies: `+52` = state `+724` (foot A down), `+53` = 0, `+236` = `+725` (foot B), `+237` = 0.
4. Footplant end (`+768` fell: `+460` was set, now clear): if `+54` (A was down) → `+464` = 10,
   else if `+238` → `+468` = 10. Both counters then decrement (when > 0).
5. Poster `sub_824E9FD8`; if on foot: walking voices `sub_824E9D10`; jump voices `sub_824E9678(on
   foot, footplant ended)`; `sub_824EBA08` (remote players only: sends message 0x2056/294 on leaving
   foot: not modeled); stores `+404` on foot, `+460` = `+768`, `+54/+55/+238/+239` = this frame's
   copies; splash `sub_824EBB58` (§2.5, not ported).

### 2.1 Poster `sub_824E9FD8`
- Materials: `+240` = state `+728` (foot B), `+56` = `+732` (foot A), 143 → 3.
- Packets: if holder `+220` (B) or `+36` (A) is empty, post (B first) `playercharacter_footstep`
  (bank `fstep_skateshoe1_sm`, constructor `sub_824B73E0`, 25 words, all clamped):
  `[0, 0, 4096, 0, 25000, 0, 0, 32767, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, eq+10]`,
  eq = eEQChain `C014A21D0FF6EDBA` = 2 (→ 12: bus 2, created). Held for the owner's life.
- run = `+408` > `729D6290FB6A8E3B` (400); run mode = `+408` ≥ `E82237E2D18B5C5F` (400) (→ the step
  selector's mode 2, else 0). Running starts near 5.9 m/s.
- Per foot with a rising edge (A: `+52 && !+54`, B: `+236 && !+238`) **and the local player**
  (`[owner+28]+72`): create the holder's four FootStep SubMix graphs if missing (`sub_82494A68`),
  stop its four slot sounds (`sub_82494B80` → `sub_824836B8` = destroy), then:
  1. surface layer 0: (bank, id) = `sub_82493E60(run, bucket +300, 0, material)`; if id ≠ −1: R =
     the sound's EQ record (`sub_82493448`), slot floats HPF = R+20, LPF = R+16, PI20 freq/gain/Q =
     R+12/R+8/R+4, slot gain = `sub_824940F8` override or R+0; apply the submix (`sub_82494550`);
     start the Splice sound into the slot's submix (`sub_82975700` + `sub_82975A60`, block
     [0, 1, 0, dt, 1, 1]); then surface layer 1 the same way (gain R+0, no override). Layer 0 = −1
     skips layer 1.
  2. step layers 0 and 1: (bank, id) = `sub_82493690(mode, material, step code +740, layer)`; the
     slot floats are not touched (the constructor's open EQ, gain 1, `sub_824D7CE8`); apply; start.
- Holder layout (`sub_824D7CE8`): +0 packet, +8 env level (f32), +12 azimuth (int), slots at
  +24 + 40k: +0 sound, +4 HPF 0, +8 LPF 96000, +12 PI20 96000, +16 gain 1.0, +20 Q 3.0, +24 slot
  gain 1.0, +28 channels 6, +32 graph, +36 node list.

### 2.2 Selectors
- `sub_82494F58(m)` = AudioSurfaceMap word 6 (`+24`), the **footstep surface** (entry 94 for m ≥ 94).
- `sub_82493E60(run, bucket, layer, m)`: s = surface(m).
  - s = 7 → bank 0 (Skate_Collisions), s = 5 → bank 1 (Skate_Metal); ids [layer 0, layer 1]:
    bucket > 1: 519/518 (s7), 323/324 (s5); walk: 959/961, 455/458; run: 960/962, 459/460.
  - else layer ≠ 0 or m ≥ 143 → −1; else `sub_824975D8(m, run, bucket)`: the material's
    AudioSurface record (class `D40CB4C0FFE45676`, key = image table `0x8302D6E8` +8, kind = +0):
    `+125` (`B1CF62EA632CF13F`) false → −1; bank = kind; kind 0: bucket > 1 → `+104`, walk `+64`,
    run `+60`; kind 1: `+84` / `+80` / `+76`; kind 2: walk `sub_824825D0` (UNCERTAIN), run
    `5432B35224E4B1C1` (no bucket test); kind ≥ 3 (none) → id 0. These are the collision export's
    `ids[5]`, `ids[1]`, `ids[4]` (same record offsets).
- `sub_824940F8(m, bucket)` (the slot-gain override): false for surfaces 5/7, m ≥ 143 or no record
  id; else `sub_824977C8` = `+48` (`37320DF00471F91A`) / 32767, bucket > 1: `9D6068AB16703650` / 32767.
- `sub_82493690(mode, m, code, layer)`: uneven = code ∉ {1, 3, 5}; s = surface(m).
  Layer 0, bank 7 (`sk8_foley`), by s = 1..7 (else −1):

  | mode | code | s1 | s2 | s3 | s4 | s5 | s6 | s7 |
  |---|---|---|---|---|---|---|---|---|
  | walk | 1/3/5 | 86 | 82 | 86 | 91 | 86 | 78 | 75 |
  | walk | 2/4 | 87 | 83 | 87 | 91 | 107 | 79 | 110 |
  | run | 1/3/5 | 107 | 102 | 105 | 105 | 105 | 99 | 101 |
  | run | 2/4 | 104 | 103 | 104 | 106 | 104 | 100 | 100 |

  Layer 1, bank 0, only s3/s4: 963/964, except run with code 2/4: 522/523.
  (All keys in the OffBoard collection; resolved with `keys.py`.)
- `sub_82493448(id, bank)`: bank 0 searches `60B043CCA6F211C3` (tCollisionSpliceAttribute: 962,
  960, 961, 959, 518, 519), others `CA945189654051D3` (459, 460, 455..458, 323, 324); the entry's
  record of class `370AF2704BFA6866` (fields +0 `411D1D4CE3AFA346` gain, +4 `DE9BF5C10AB2C866` Q, +8
  `7D3294EC1C443CBD` PI20 gain, +12 `805ABC23217FAC46` PI20 freq, +16 `79D8AB26EA217567` LPF, +20
  `4E0F00838F472B4A` HPF); no entry → the `default` record (0.09, 3, 1, 96000, 96000, 0). Values in
  `FootstepTuning::default` (e.g. 959: gain 1, HPF 200, LPF 7000, PI20 5200 Hz × 0.1 Q 0.5; 962:
  gain 0).

### 2.3 Walking and jump voices
- `sub_824E9D10` (on foot), per foot rising edge: stop the foot's voice, start `sk8_foley`
  (v = |COM v| > 7.5 → 64, > 2.5 → 63, else 62; Clothing `E12AF885D3C3A168` = [7.5, 2.5], ids
  `9D6D2863CFE908C4`/`EC3399A49055DD8D`/`6B61C043E53C44CB`) into SFX Master (`[[0x830CFDBC]+44]`),
  `+432` (A) / `+428` (B); with local && on foot && state `+308` also `Skate_Collisions`
  1123/1122/1121 (`67717E2388A836ED`/`E420F7DD48E01E0E`/`F0292A62D280EB40`) → `+440` / `+436`.
  Start block [0, 1, 0, dt, 1, 1].
- `+308` = OffBoard 311 = the board held in hand (packed record `+164` bit 1, `sub_827A1B78`). Retail
  sessions: ~half the walking steps carry the Skate_Collisions layer.
- `sub_824E9678(on foot, footplant ended)`: off-feet = on foot && `+718` (OffboardAir) rising
  (`+405` last); falling = COM vy (`+100`) ≤ 0 → `+456` = 1 (else 0; 2 at start), began = falling
  && old `+456` == 0; apex = `+444` held && began; hippy = `+372` rising (`+452`).
  - off-feet || hippy || footplant ended: stop `+444`, `+472` = `+304` (jump bucket), start
    `sk8_foley` off-feet ? 65 : bucket 1 → 69, 2 → 71, else 67 → `+444`; `+476` = off-feet.
  - apex: stop `+444` and `+448`, start `+476` ? 66 : bucket 1 → 70, 2 → 72, else 68 → `+448`.
  - block [0, 1, 0, dt, local ? 1 : 0, 1]; SFX Master. Retail: 65/66 ×10–15 per session.

### 2.4 Update `sub_824E9628`
1. `sub_824EAEA8` (both packets present): per packet (A = `+36`, B = `+220`) w0 32767, w1 raw(0)
   0..65535, w2 pitch(1) 0..8192, w4/w5 vf64(4)/(5) 0..25001, w6 level(6), w7 level(2), w8 foot down
   0/1, w9 vertical word (A `+420`, B `+424`) 0..1000, w10 xz word (A `+412`, B `+416`), w11 =
   (countdown > 0 || `+444` held), w12 `+300` 1..4, w13 trunc(`+796`) 1..99, w14 `+408` 0..1000, w15 1,
   w16 surface(material) 1..7, w17 `+740` 1..5, w18..w23 `636464FBAD0D71A3` = [32767, 10000, 15000,
   25000, 32767, 28000]; redeliver A then B. Then (local): level = level(7) / 32767, or level(11)
   when `sub_824940F8(foot A's material, bucket)` holds (shared by both feet); holders' env =
   level(8) / 32767, azimuth = raw(0); `sub_82494C08` per holder, slots 0, 2, 1, 3: dead → destroy;
   else block [slot gain × level, pitch(1) / 4096, 0, dt, 1, 1].
2. `sub_824EA9C8`: walking foley `+432`, `+428`: [level(3) / 32767, pitch(1) / 4096, raw(0) ×
   360/65535, dt, local, 1]; dead → freed. `sub_824EAC38`: `+440`, `+436` at level(12).
3. `sub_824E9AB8`: take-off `+444` at level(9) (dead: **kept**, not freed; it gates w11 and the
   apex), apex `+448` at level(10) (dead → freed).
4. `sub_824EBE78`: the splash sounds (levels 13/14/15): not ported.

### 2.5 FootStep SubMix (`sub_82494188`, per foot slot)
Mono graph: `Sub0 → HI20 → LI20 → PI20 → Sen0 (→ env bus input `[[0x830CFDEC]+52]`) → Pn21 (1 → 6) →
Sen0 (6 ch → SFX Master [[0x830CFDBC]+44])`. `sub_82494550` (at each start of the slot) posts HI20 =
slot+4, LI20 = slot+8, PI20 freq/gain/Q = slot+12/+16/+20, Sen0 #1 level = holder+8, Pn21 azimuth =
holder+12 × 360/65536 (`0x822F88E8`). So env level and pan are those of the last update, latched per
start. Port: `SpliceHost::set_submix(Some(Submix))` before each foot start (host default ignores it);
the route carries `owner_env` = the env level. Read 2026-10-02: Sen0 #2 is never posted = class default 1.0
(`ATTRIBUTE_SETGAIN`; recomp SEND lines: 1.0 on all 8 graphs); Sen0 #1 also starts at 1.0 until the first post;
Pn21 constructor args = descriptor defaults (front 30, side 110, rear 150) with arg 2 (normalization/law) overwritten
by 0.0 → law gain 1.0 (as the voice open).

### 2.6 Splash `sub_824EBB58` (not ported)
State `+811` in water (`+477` latch, `+480` time in water += dt), `+812` entered (once per `+478`,
only with state `+224` clear): stop `+484`, Skate_Collisions id `35D3B06292CDA10B`, or
`C17485220849574D` when `+480` ≥ `9CD13431903E3719`; `+813` (once per `+492`): id `BA81E93AE985D1C7`
→ `+488` (class `923CCB46EF5BF5BA`, collection `B2BD1F28DDE601B0` = material 92). Played through
`sub_82497F48` (collision splice object) into SFX Master. Retail ids 1197/1198 ("splash").

## 3. Audio-state inputs (bridge `sub_824B0DA8` from the packed record `sub_827A1B78`)

| state | retail source | engine (read-only survey 2026-10-02) |
|---|---|---|
| `+724` foot A down | rec+152 bit 0x20000000 = OffBoard 307 ‖ Air 450; ‖ `+334` ‖ `+336` | `p.off_board.flags_306_307[1]`, `p.air.footplant_right_450`, push foot (State55 && State57), brake flag(52) |
| `+725` foot B down | 0x10000000 = OffBoard 306 ‖ Air 449; ‖ `+333` | `flags_306_307[0]`, `footplant_left_449`, State55 && !State57 && State56 |
| `+732` / `+728` | OffBoard `+56` / `+52` u16 tags (Air `+224` while Air 448) & 0x7F − 1 | `skater.offboard.feet.hands[1/0].word_80` (`flag_84` valid) or `processed.right/left_surface_2600/2596` |
| `+288` / `+284` | max(\|x\|, \|z\|) Skeleton `+240` / `+224` | `foot_physical.output.world_velocity[1]` / `[0]` ✓ |
| `+296` / `+292` | \|Skeleton `+308`\| / \|`+324`\| = \|y\| of the **angular** velocity (body `+48`) of assembly parts 20 / 16 | ✓ (2026-10-02) \|`skeleton.bodies()[20/16]` angular_velocity.y\| |
| `+740` step code | `sub_827729B8` (§3.1); Skeleton `+144` / `+160` = physical pose translations of toe parts 15 / 19 (Skeleton 9024 / 9280) | ✓ `StepCode` over `skeleton.record.pose[15/19][3].y` |
| `+796` | Interaction+0 AudibleFootStepStrength | `animation_input.extra.footstep_strength` (engine comment: held 2.5–4; retail walking is < 2 → w13 = 1; check a real-play log) |
| `+300` landing bucket | §3.2 | `LandingBucket` helper over `p.reckoning.vector_16[1]`, `p.filtered_state_0 == 7` |
| `+304` jump bucket | `sub_82772D30`: jump strength vs `468752B0BEE65CDB` = [0.45, 0.75] | `animation_input.extra.jump_strength` (host writes 0 today) |
| `+768` | rec+156 bit 7 = Air 448 (FootPlant) | Air 448 (FootPlantManager; check `p.air`) |
| `+718` | rec+152 bit 24 = filtered state 7 | `p.filtered_state_0 == 7` ✓ |
| `+716` | rec+152 bit 30 = category 500 | state 500 ✓ |
| `+308` | OffBoard 311 (board held) | `p.off_board.flag_311` ✓ |
| `+337` push stroke | State56 (rec+148 0x20000000) | `state_flags[56 − 52]` ✓ |
| `+335` push plant edge | State55 rising vs old `+333/+334` | `push_trigger` (host) ✓ |
| `+328` | \|Skeleton `+288`\| = the angular speed of part 23's body | ✓ |
| `+672` | 0.25 × (572 + 568 + 564 + 560): \|record velocity − Skeleton16176\| of parts 8, 4, 21, 17 | ✓ |
| `+528..+548`, `+560..+580`, `+593` | Collision `+80..` block (`sub_82BD60C8`, via the conditioner `sub_82773298`, which maxes only the first 8 floats over 4 frames): +32.. = SkeletonCollision region tangent speeds (+944 + 4i), +64.. = their tags (+976 + 4i), +97 = byte 4009 (face point contact) | ✓ `collision_feedback.regions[i]` (0 without a part) / `specific[1].current` |
| `+688` / `+689` | Skeleton `+602` / `+603` (§4) | inputs exist: `foot_ik.state.frames[2/3].board[3]`, deck settings, `p.off_board.flag_311` |
| `+676` / `+677` | bail / Skeleton 599 | ✓ (existing fields) |
| `+372` | hippy jump | host: scorable 234 (engine also `p.ground.hippy_jumping_322`) |

### 3.1 Step code `sub_827729B8` (audio conditioner, writes B40 211..215)
Tags `c+738` ← OffBoard+56, `c+736` ← OffBoard+52 when non-zero; B40+211 = ((`c+736` >> 7) & 31) == 8
(physics class 8). `c+740/741` = OffBoard 306 (now/last), `c+742/743` = 307. On 307 rising `c+720` =
Skeleton+160 (vec), on 306 rising `c+704` = Skeleton+144. d = y(+720) − y(+704): |d| ≤ 0.07
(`0x821BCD64`) → 212 = 213 = 0; else if 307: d > 0 → 212 = 1, else 213 = 1; else if 306: d ≤ 0 →
212 = 1, else 213 = 1 (bytes kept otherwise). Code = 212 ? (211 ? 2 : 4) : 213 ? (211 ? 3 : 5) : 1.

### 3.2 Landing bucket (`sub_82772B88` + bridge)
Ring of 4 COM vy (Reckoning+20); a = |min(0, r0..r3)| (fsel chain); a > 3.45 → 4, > 2.6 → 3, > 1.5
→ 2, else 1 (`7385078DD3C063BA` = [1.5, 2.6, 3.45]). Bridge: 1 if trick active with audio trick ≠
31, or `+720` == 0 (`+720` = 20 while `+718`, else −1 per frame; checked before its update).

## 4. Hands on the deck (Contacts `sub_824B85B0`)
- Skeleton `+602` / `+603` (`sub_82BF20C8`, before `sub_82BF22A0` in Skeleton::FillPhysOut
  `sub_82BE1AE8`): SkeletonIK `+1840` / `+1904` = the translation of `+1792` / `+1856` = the engine's
  `FootIkState.frames[2].board` / `frames[3].board` (hand limbs, parts 3 / 7: the animated target
  relative to the animated board; updated only while the limb is on the deck or blending) strictly
  inside (DeckWidth/2 + p.x, 0 + p.y, DeckFrontEndSize + DeckMidLength/2 + p.z) with p =
  `physics_skeleton` `A28E50D30B0506A4` = (0.03, 0.03, 0): the same test as 600/601 (toes, pad
  `FootOnDeckPadding`).
- Packed record bits 0x2 / 0x4 of +148 → bridge `+688` / `+689`; both cleared when on foot (category
  500) without the board in hand (OffBoard 311).
- Rising `+689` then `+688` → `sub_824B8310(hand, +332)`: stop `+372+8·hand`, start Skate_Collisions
  1124 (in the air) / 1125 (ground) (`A7B32EC5FF4997F5` / `F17EEB71F18CEAF3` in `923CCB46EF5BF5BA` /
  `E0C3B44AB44F7B90`); falling → `sub_824B8448`: stop `+388+8·hand`, start 1126 (`A7B7AE2A6C25670F`).
  Route eEQChain `ED52262DABB5DE4C` = 2 (create = local), block [0, 1, 0, dt, 1, 1].
- Update `sub_824BF728` (Contacts update chain after `sub_824BF268`): on sounds [trunc(level(10) ×
  0.5) / 32767, pitch(11) / 4096, raw(0) × 360/65535, dt, 0, 1], off sounds × 0.3
  (`25DCB888413D570D` = [0.5, 0.3]); dead → freed.
- Retail: 1125 ×43–53, 1126 ×47–53, 1124 ×3–5 per session (board pick-up / put-down by hand). The
  interim layer's "1126 = foot on deck" is a different, wrong mapping.

## 5. Clothing (`sub_824DBB68`: cloth falls, push foley, body slide)
- Cloth falls `sub_824DBF10`: k = 1 / 5.0 (`0A9A9BD1150FE838`); a = trunc(k × `+328` × 1000), b =
  trunc(`+672` × k × 1000); `+44` = max(a, b). Held: release on `+677` or `!+676`. Not held: post on
  `+676` rising `c_cloth_falls` (Foley_Cloth, `sub_824B72D8`, 10 words): [0, 0, 0, a 0..1000, 25000,
  0, 0, 0, 0, eq `E633C8F009CAEFFC` = 5]. Update `sub_824DCA48`: w1 raw(0), w2 pitch(1) (Clothing out1
  is a volume output read as pitch → 8192: retail's Foley_Cloth pitch p50 1.81–1.83), w3 `+44`, w0
  32767, w7 level(2).
- Push foley `sub_824DBBB8`: stroke = `+337` rising: if no `+48`, start `sk8_foley` 73
  (`6FD273157742AC4B`); held `+48` stops on `+335` or bail; on `+335`: restart `+52` with 74
  (`616CE02AA10D596F`); `+52` stops on bail. Route eEQChain `AEA5BA7E64515945` = 0 (create = local),
  block [0, 1, 0, dt, local, 1]. Update `sub_824DC7D8`: [level(4) / 32767, pitch(3) / 4096, raw(0) ×
  360/65535, dt, local, 1]; dead → freed.
- Body slide `sub_824DC0E8` / helper `sub_824DC2B0`: sliding = any |`+528 + 4i`| > 0 (i 0..5); flag =
  sliding && `+593`; tag = part 0's, overridden by part 1's, else the first non-zero of parts 2..5;
  speed = trunc(`+212` / 4.5 × 1000) (`787171ECD02DBBC3`); material = tag − 1 (0..143 else 143); type
  = 143 → 2, flag → 4, else AudioSurfaceMap word 10 (`+40`). Held: release unless speed > 150
  (`746EA8EF187E1571`) && sliding; else post when sliding && speed > 350 (`B021DB338B89D0F2`)
  `c_body_slide` (Bodyslide, `sub_824B7070`, 12 words): [0, 0, 0, speed, 25000, 0, 0, 0, type 0..4,
  bail 0..1, 0, eq `4A022FEF9905D8F4` = 7]. Update `sub_824DC578`: w0 32767, w1 raw(0), w2 pitch(6),
  w3 speed, w7 level(5), w8 type, w10 trunc(`+328` / 8 (`5D2F244E82CF7255`) × 1000), w9 bail.

## 6. MixMap outputs read
OffBoard (`0x40010090`): raw 0, pitch 1, filters 4/5, levels 2, 3, 6, 7, 8, 9, 10, 11, 12 (13–15 the
splash); free skate on foot: 2 = 9202, 3 = 8972, 6 = 1789, 7 = 2593, 8 = 1933, 9 = 3857, 10 = 9202,
11 = 14602, 12 = 11896 (in0 does not change them). Clothing (`0x40010060`): raw 0, pitch 1 (a volume
output), 3, 6, levels 2, 4, 5. Contacts (`0x40010010`): level 10, pitch 11, raw 0.

## 7. Retail comparison (per-voice GAIN p50, `tools/retail_voices.py --csv`, sessions 163809 / 164620)
The report.md per-container "audible" medians mix simultaneous starts (e.g. step layer 82 shows
0.21–0.26 there); the per-voice joins are the reliable measure. Ours = `tests/offboard_levels.rs`.

| sound | samples | retail p50 | ours |
|---|---|---|---|
| walking foley 62 | 94..97 | 0.387 | 0.387 |
| walking foley 63 | 103..107 | 0.244 | 0.244 |
| step layer 82 | 69..75 | 0.011 0.015 0.024 0.022 0.056 0.076 0.110 | 0.007–0.011, 0.013–0.018, 0.021, 0.020–0.024, 0.020–0.054, 0.073–0.077, 0.103–0.112 |
| board in hand 1121–1123 | 30..34 / 94..100 | 0.22–0.34 / 0.169–0.190 | 0.19–0.28 / 0.155–0.182 |
| metal surface 416 (time-matched to its 59 footstep starts) | Skate_Metal | 0.082 | 0.082 |
| push foley 73 / 74 | 0..7 | 0.176–0.260 | 0.257 |
| hand on 1125 / off 1126 | 427..432 / 422..426 | 0.133–0.160 / 0.071–0.081 | 0.125–0.153 / 0.062–0.070 (−1 dB) |
| footstep patch, w13 = 1 sets | 30..34, 45..49, 119..123, 158..162 | 0.070–0.083, 0.070–0.077, 0.026–0.040, 0.054–0.072 | 0.058–0.080, 0.052–0.077, 0.060–0.073 (+5 dB), 0.048–0.056 |
| Bodyslide | 0..4 | 0.000–0.035 | 0.023–0.027 (guessed slide inputs) |
| Foley_Cloth | 0..7 | 0.017–0.052 | 0.062–0.075 (guessed +328/+672) |

Footstep patch sample sets by w13 (probe `footstep_patch_sample_groups`): 1 → 30..59, 119..126,
141..145, 154..167; 2 → 6..9, 168..171 (retail: these open at gain 0.000); 3 → 10..17, 172..179; ≥ 5
→ none. Surface w16: 3 → 134..140, 150..153, 180..183; 4 → 18..21, 134..153; 6/7 → 0..2, 127..133,
146..149.

## 8. Open / UNCERTAIN
- (Closed 2026-10-02) Skeleton `+144` / `+160` = toe parts 15 / 19 pose translations; `+304` / `+320` = angular
  velocities of parts 20 / 16 (see §3).
- Kind-2 walk id (`sub_824825D0`); never reached on the disc (+125 clear on materials 102..106).
- FootStep SubMix: rendered since 2026-10-02 (`bus/submix.rs`); Sen0 #2 = 1.0 and Pn21 law 0 read from the build (§2.5).
- AudibleFootStepStrength scale in our engine vs retail (selects the patch's sample sets).
- Splash (`sub_824EBB58` / `sub_824EBE78`) and remote players (`sub_824EBA08`) not ported.
- (Closed 2026-10-02) Ragdoll slide records and limb speed are published (§3).
