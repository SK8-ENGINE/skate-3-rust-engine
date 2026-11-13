# EA AEMS (Skate 3 audio runtime): reference notes for the port

## Native-port spec index (2026-10-02)
| Spec | Status |
|---|---|
| `rwaudio-prior-work.md` | done: prior art (nfsmw CC0, BurnoutDecomp b5-decomp/BP-Decomp, tw2004 CC0), licenses |
| `aems-evaluator-spec.md` | done: ABKC/MOIR layouts, runtime, 40 opcodes, timing, RNG, test plan; **PoC op 5 bug** |
| `aems-voice-graph-spec.md` | done: SndPlayer1 … Send DSP, buses, output stage, validation |
| `mixmap-spec.md` | done: MXB format (247 controllers), evaluation math, Ambience/Emitter outputs, ducking; decoder + reference evaluator match PR #4 on 73,800 cells |
| `grain-player-spec.md` | done: grains.big format, surface→grain map, GrainPlayer pick/scheduler/fades, chains, validation hooks |
Plan and order: the native-port plan (doc 11, "Native audio port"), incl. the "Modular and moddable" design goals.


**Superseded in detail by `aems-evaluator-spec.md` (2026-10-02): official op names, full op semantics, and
corrections (§11 there: PR #4 op-5 handle bug, 17/18 = Mux/Demux, 19/20 n-ary min/max, 38 ControlClass,
header +8 = platform + module count, tick scale 31.999998).**

Knowledge only, from others' hard-won work; never copy their code (it transcribes game code). Sources:
- Upstream PR #4 (SK8-ENGINE/skate-3-rust-engine, commit 368a433): `crates/skate-audio-core`, `crates/skate-audio-formats`,
  `docs/audio-player-sounds-handoff.md`, `docs/player-audio-retail-drivers.md`, `docs/audio-object-registry.md`,
  `docs/implementation-plan.md`, `crates/skate-game/src/player_audio/`. Read from a local clone.
- Upstream PR #1 (`mx/audio-engine-vehicles`): read from a local clone (report below).
- sk8Audio (andrewnakas/skate3-audio, NO license → reference only), branch `phase0-measurements`:
  `docs/audio-banks.md` (canonical bank write-up), `execution-trace.md`, `rw-audio-core.md`, `rw_audio_structs.h`.
  Read from a local clone.
- Our own measurements in the recompilation (summarized in doc 11).
PR #4 docs ≠ code in places (slots 19/20 claimed ported but missing; setters only in authored.rs); trust code.

## .abk (ABKC): big-endian, offsets from bank start
Header: +0 "ABKC"; +0x08 variant (0x05030001 ×369, 0x05030002 ×7); +0x0A u16 input-record count; +0x14 size;
+0x18/+0x20 S10A sample-bank offset (twice); +0x1C first record (0x5C); +0x24 S10A length; +0x30 code fixups
(empty on all banks); +0x34 rebase list (add bank base; 4,656 words); +0x38 exports (count, 12-byte entries:
target slot offset, record offset, kind word; record = u16 project id, u16 name id, NUL name; kind top byte
selects .csi table: 0→2, 1→1, else 0). Run time: +60 bank id, +64 S10A ptr, +72/+76 voice-open args, +80/+84 list.
S10A: magic, 0, count, offsets rel. to tag; 0xFFFFFFFF = unused (used slots are a prefix).
Input record (60 B + 4/entry): +4/+8 bound symbol + {name_id,generation}; +12..+27 listener node (fn 0x82B1DAD0,
ctx = record); +28/+30 u16 live/capacity; +32 u16 #28-byte variable subscriptions (table-2); +34 u16 #broadcast
subs; +36 u8 #voice objects; +37 has release cb; +38 has payload copy; +39 #held-message objects; +40 program
offset; +44 template offset; +48 template size; +52 offset of back-pointer triple; +56 live list head;
+60.. (+36 + +39) u32 offsets into the template (first +36 get the bank pointer at their +0).
Instance = copy of template: +0/+4 live links; +8 evaluator node (program at +16, operand block at +20 =
instance+24); from +24: optional 20-B release entry; N×28-B subscriptions (value at +24); payload entry (node,
fn, ctx, u8 word count at +16, words from +20; size (count+5)*4); broadcast subs (binding, node, u8 count +24,
arrival flag +25, words from +28; (count+7)*4). Triple: record, instance, post node; +12 = "end me".
Payload word i (no release/subs) at block+20+4i.

## Program grammar (all 385 programs verified)
Records until opcode 255: u8 opcode, u8 pair count, 2 unused, pairs × 8 B (source, destination), s32 block
advance. After the op: per pair, source −1 → store op result low word at block+dest, else copy block+src →
block+dest; then block += advance (64-bit arithmetic, pair count re-read each iteration). Opcodes < 40.

## Evaluator tick
Per audio block δ = 256/48000. Frame count = steps of δ until 1.0/denominator (denominator 30.0 after game
init) → 6 blocks (~31.25 Hz); tick scale = count × δ × 1000 = 32.0 ms. Countdown at 0x830775E4; on zero walk
the evaluator list 0x83036F4C: per record, pure ops then host ops (4/5/27/39). Random: global six-word counter
0x830775F0 (not reproducible run to run).

## Opcodes (table 0x82FD3600; PR #4 file refs in eval/)
0 return +16 then zero · 1 return +20 · 2 return +24 · 3 return +0 then zero · 4 end instance (if triple +12) ·
5 clamp (+8 flag, +9 count, [min,max] from +12) and broadcast to subscribers · 6 stepping cursor (low, high,
current, s8 step, enabled, probe; wraps) · 7 random base + draw % range (cached while disabled) · 8 shuffle bag
(byte/halfword, wrap) · 9 weighted pick (s8 weights summed past draw % 100) · 10 hysteresis latch (+17 edge
flag) · 11 one-shot float timer (steps by tick scale; fires → −1.0, returns 1) · 12 first non-zero flag → value ·
13 any of n non-zero · 14 multi-segment envelope (modes 1 start/run, 2 hold, 3 jump/run; {duration, target}) ·
15 sampled curve (s8/s16/s32; nearest at scale 1.0 else linear) · 16 word delay ring (length = delay/tick) ·
17 stack top (1-based) · 18 stack push/clear · 19 counted minimum? (NOT ported) · 20 counted maximum? (NOT ported;
used by Rolling_Surfaces, RocksBounce) · 21 round(scale × product of n) half away · 22 sum n · 23 sub · 24 mul
(64-bit) · 25 div (0 on /0) · 26 rem · 27 VOICE OP · 28 oscillator (quarter-sine table 0x82FD36B8, pulse, ramp,
triangle) · 29 ramp to int target over duration (tick scale, 1/4096 unit) · 30 capped sum · 31 max(b−c, floor) ·
32 capped multiply · 33 min · 34 max · 35 round(f32 × int × int) · 36 add · 37 return +25 flag and clear
(broadcast arrival) · 38 unknown (not ported) · 39 latch setpoint, notify table-2 listeners with clamped value.

## .csi (MOIR) projects
Counts u16 at +0x0A/+0x0C/+0x0E, project id +0x10, tables from 0x28. Tables 0/1: 12-B (listener head, name, u16
name id, u16 generation); table 2: 16-B (listener head, VALUE, name, id, generation). Install: rebase names,
fresh generation (global u16 0x83083CD0), table ptrs at +20/24/28, link project list 0x830BBE50. Lookup: pass 1
matching project id, pass 2 all; match name id + string; hit writes {record ptr, id|generation}; miss −5.
Table 1 = objects gameplay posts to.

## Post → instance
Install all .csi, then each .abk (rebase, resolve exports, hang each input record's listener on its symbol,
write bank ptr into template voice objects). Game object table 0x8302D4A4 (72 entries); handle slot of object i
= 0x8302EE28 + 8i. Post: validate slot (neg id → id; null → −6; stale generation → clear, −3); alloc 16-B post
node (symbol, refcount, payload-callback list +8, release list +12); call every listener, then payload
callbacks. Spawn (only while live < capacity): copy template, triple, live list, node program/block, release cb,
subscriptions (copy current value), payload copy, broadcast subs, push node on evaluator list; then payload
words copied in. Held messages: gameplay rewrites payload and re-runs payload callbacks each 60 Hz frame (seams
twice per frame). End: op 4 when triple +12 set, or game release → release cbs set instance +16 → program fires
op 4. Unmapped broadcast handle (e.g. literal 1 in Rolling_Rattles template) = "no holder yet".

## Voice op (27) and graph
Voice object: +0 bank; +4 descriptor table (count, 12-B descriptors); +8 open voice; +12/+13 latched/previous
state; +14 #param records; +15/+17 copy-back flags; +20 descriptor index; +24 requested state (0 off, 1 on,
2 paused); +28.. 12-B param records {u8 id, applied, wanted}. Op: clamp request; on change: 0 release, 1 resume
or open (index clamped; descriptor first u16 0xFFFF = no sound), 2 suspend; while on push changed records,
query voice (dead → deactivate; copy remaining/elapsed time back); poke.
Open: sample = S10A entry (index+3) + base; descriptor byte 2 = gain % (×0.01 → player float); bytes 3–8 <<8 =
panner angles; bank +72, bank +76 + descriptor +8.
Property ids: 0 pitch = value/4096 (resample ratio); 2 master level /32767; 5 first send /32767 (× master);
8 dry level /32767 (Gain = master × this); 6 low-pass raw (25000 = open); 7 high-pass raw; 3 pan value ×
360/65536°; 11 effect send /32767 (if effect bus); 1, 4 ignored; clamps: 0,6,7 → 0..65535; 2,5,8 → 0..32767.
Routing records (id ≥ 9) at open: 0–7 material bus n; 10–17 bus n−10 (on demand); 512/2048 fixed buses;
4096/8192/16384 effect bus (next record 1 = enable, next = level /32767; 16384 = second effect bus).
Graph (fixed): SndPlayer1 → Rechannel → Resample → HighPassIir2 → LowPassIir2 → [Send if intermediate target]
→ Gain → [Send if effect bus] → Pan2D1 (6 ch) → Send (6 ch, output bus). Mono: pan property 0 (mode 1).

## Player objects (slot = handle index; payload words per docs/player-audio-retail-drivers.md §4–5)
Conventions: vf52(0) azimuth 0..65535; vf56(n) pitch cents → ×4096 (0..8192); vf60/64(n) 15-bit MixMap levels;
25000 = open filter. Inputs from the audio-state bridge (drivers §1, audio_state.rs).
- 0 Class_foot_drag FOOT_DRAG (15 w) · 1 Class_wheels_skid WHEEL_SKID_BANK (18 w) · 6 Class_Seams Seams_Bank
  (20 w; four held packets, one per wheel: w7 trigger pulse, w8 speed, w10 surface, w13 pattern class, w14 wheel)
· 7 Class_Squeaks Brd_Squeaks (11 w) · 9 Class_Treatment Treatments (22 w, posted once & held: w7/w8 air times,
  w9 jump height, w10 time scale 500) · 12 SenseOfSpeed_wind (13 w) · 13 SenseOfSpeed_rattle (11 w, w3 intensity)
· 15 c_board_slide board_scrapes (12 w) · 21 c_body_slide Bodyslide (12 w) · 22 cloth_trick Foley_Cloth (11 w)
· 23 c_cloth_falls (10 w) · 24 playercharacter_footstep fstep_skateshoe1_sm (25 w) · Class_Flips (28 w) ·
  Class_grind GRINDS (17 w) · boot utilities 16/20/38 (1-word posts).
- Class_rolling (PatchBank_Rolling_Surfaces/_Objects/_SpiderCracks/_RocksBounce): post w2 4096, w3 speed
  (0..10000), w4 selector (0..15), w6 surface (0..13), w9 25000, w11 32767; held updater w0 32767, w1 azimuth,
  w2 pitch(8), w3 speed, w6 surface (13 when both trucks lifted while grinding), w7 manual/brake, w8 level(19),
  w9/w10 filters 17/18, w11 level(7)/(9). Surfaces 7, 8, 10–13 post selectors 1, 2, 10, 12, 11, 9.
  **Retail's continuous rolling = the grain player; Class_rolling is sparse one-shot texture.**
- Rolling_Rattle_Class: w3 speed, w4 surface code 1..5, w6 1, w8 25000, w10 32767, w11 8; updater w2 pitch(3),
  w7 level(16), w8/w9 filters 14/15, w10 level(6); posted on push trigger state byte 335.
- Contacts (SFXObj_Contacts): Splice (SPLC .bnk) one-shots: formats splc.rs, selection/gain/pitch
  authored/oneshot.rs, game side player_audio/contact_voices.rs. Our decoded rule: doc 11 (recomp measurements).
- Collision (SFXObj_Collision / CSTATEMGR_Collision): 10 LRU slots, two voices per message (one per material),
  material → Skate_Collisions.bnk / Skate_Metal.bnk (collision_states.rs).
- Missing in PR #4: landing dynamics (Contacts MixMap inputs not written); global random not reproducible; "G"
  global writer; wheels.big and jet grain partial; Class_rolling broadcaster for placeholder symbol 1.

## Validation tools in PR #4 (ideas to mirror, not code to copy)
grind_instance example (post payload, tick 256/48000, redeliver traced updates, GRAPH/DISASM/WATCH); wheel_repro,
render_player_audio; retail capture tags ST/PO/UP/RL/BD/GP/GL/VF/MC + audio_capture_extract/verify/compare
tools; compare medians in narrow speed bins, never session averages.

## PR #1 (mx/audio-engine-vehicles): what it adds (report 2026-10-01)
PR #1's skate-audio-core = PR #4's (PR #4 is a later superset: adds measured transfer functions §11, defect 13
"Collision switch = input 15 bit 0 on controller 60030000"). PR #1 adds the game side:
`skate-game/src/player_audio.rs` + `player_audio/**`, `physics/audio_observation.rs`, `skate-data/src/audio/*`,
`skate-audio-formats/*`, docs player-audio-retail-drivers.md (DENSE: constants, field layouts, packet tables),
audio-object-registry.md, audio-player-sounds-handoff.md, player-audio-implementation-plan.md,
wheel-audio-handoff-2026-09-17.md, engine-defects.md; tools audio_capture_extract/verify.py, audio/landing_levels.py,
audio/skid_scrub_levels.py.
Architecture: (1) every gain/pitch/pan word is an output of EA PathFinder 5.03 **MixMap** (data
`data\audio\MixMapSK8.mxb`, 25,252 B, 14 sections; port matches capture on 99.9991 % of 84 M words);
(2) rolling = **grain player** over grains.big owned by SFXObj_SkateBoard (Class_rolling only sparse texture);
(3) one per-frame **audio state** (bridge sub_824B0DA8; record *(0x82083C38)+0x2F070 +240 +544*player).
Object model: CSTATEMGR_<X> → CSTATE_<X> slots → SFXObj_<Y> components; controller key
0x40000000 | cat<<16 | index<<11 | kind<<4. Per 60 Hz frame: bridge + state-controller inputs (sub_824B19C8) →
every component process (vtable+36: write inputs, post/release) → one MixMap evaluation → every component update
(vtable+40: rewrite held packets from mixer outputs, redeliver).
Audio-state fields: 208 ground speed; 212 |COM v|; 236 air time; 240 time to land; 260 jump height; 232 slip;
264 deck tilt; 332 known air; 333-335 push edges (335 rattle trigger); 336 brake; 339 manual brake; 340 balance;
341 grinding; 343/348/352 trick ids; 448-467 per-wheel touchdown impact + landed latch; 468 jump velocity;
496-611 ragdoll contacts; 620-660 per-wheel material (143 none) + seam pattern; 676/677 bail/end; 724/725 foot
down; 780 loose board; 796 footstep strength. Conditioner rules: drivers §6 / audio_state.rs.
Levels (music off): rolling -22.9 dBFS; takeoff -5.9 (+17 dB over bed); landing +0.4 (+23.3 dB).
Grain bed (drivers §3): 13 .grain soft/hard per surface; per-surface vault record (4x4 Bezier, max km/h 55-74,
boost); position = Bezier(clamp(v*3.6/maxKmh)), player B 0.1 behind; gain level(1)/32767*(1-max(turn,brake
slew)); pitch(3)/4096; GrainParams A (0.1,0.2,0.1,1.6,0.05) B (0.2,0.1,0.2,1.5,0.05); bus chains incl. SSB
frequency shift, HS 5 kHz; push envelopes; seam gain envelope; distortion send; gain wobble.
Contacts: pop (selector by jump velocity 0.25/0.42; Skate_Collisions ids 0x449-0x44B / 0x44F-0x451 by surface
category); landing = voice 1 fixed 0x447, voice 2 2x2 ladder (deck material × air >= 0.75 s) 0x35C-0x35F,
voice 3 class voice 3*kind+class; Contacts input 2 = 0/16000/32767 by class → output 15 = 2584/3103/3650.
Collision manager: 10 LRU slots, 48-B message (2 materials, tiers, pos, levels, flags), tier gate
10000/20000/32767; material table 0x8302D6E8; impact bands floor 0.005, 0.5, 1.0, cap 2.0.
Output: native 6 ch 48 kHz, downmix front + 0.707 C + 0.5 surround, OUTPUT_GAIN 0.6.
Formats: XMA2 via ffmpeg, pad chunks to 2048-byte packets (else 64-sample ticks); grain seek table; splc walk.
Unknowns: global G *(0x83083C38)+0x2FCB4; pop bank pick sub_824B9AD8; Contacts inputs 3/6/10/17/18/21; Dac stage.
Gotcha: rustc 1.98.1 -O miscompiles a hand-written clamp (use .clamp()).

## CORRECTION (2026-10-02, from retail code: sub_82976860/sub_82976360/sub_82975B08): SPLC member fields
PR #4 swapped them. **Member +4 = gain** (amplitude steps 0.7071, 1.4142, 0.315 … 11.31), random level spread
+44; **member +8 = pitch** (semitone ratios 0.9439, 1.0595, 0.5, 2.0) + random +48. Voice gain =
member+4 × spread(+44) × block[0] × fade envelope (+24..+40, mostly zero); pitch = member+8 (+48) × block[1].
Record header (sub_82975B08): block[0] ×= record +8 (record gain, e.g. 0.5 landing impact); block[1] ×= container
random (record +12/+16, pitch). block[0] = level rewritten by the owner every frame; block[1] = an owner output in
cents (1200/octave). **Our own main-checkout `audio_formats.splc_patches` / game_audio play_record read +8 as gain
too; fix in Phase B.**
Contacts (sub_824BE1B8 per frame): pop +60 = trunc(level(2)×K[sel])/32767 (vault 8BF3668CF7994CD4 / 212B6F75D11AA39B);
pop roll +96 = level(7); ollie voice 0x448 +52 = level(12); landing impact 0x447 (container 1095) +56 = level(13);
touchdown voices (sub_824B8D48, 4 slots +140) = trunc(level(3)×K[cat][3·kind+variant])/32767 (vault arrays
CEA5AFA8BA170B07, 951AD53718030327, 068C8C5EBEC1B45F, 6823F4910A1882AD); pitch = output 1 in cents; output 15 =
env send of the touchdown owner bus (not dry gain). Landing sound = touchdown voices from the per-frame wheel handler
sub_824B86E0 (per-wheel latches +132..135, newly-down count → kind, class 2 = variant 1 + second voice variant 2),
manual landing sub_824BB330 (kind 4); MixMap: only Contacts input 2 (landing class) moves outputs (3: 6590/7913/9309).

## Corrections from the voice-graph spec (2026-10-02; `aems-voice-graph-spec.md`)
- **`SKATE3_AUDIO_CAPTURE`** records the recomp HOST's stereo downmix (0.4·(FL + SL + 0.5·C) per side),
  not the game's output stage. Absolute dBFS from captures are in capture space. PR #4's −22.9 dBFS rolling
  was measured on the native 6-channel output, so compare **relative** levels (takeoff +17, landing
  +23.3 dB over the bed, ±1.5 dB).
- **Trace columns:** `PLAY` "level" is a play counter, not a level. `SEND` per-channel values are each
  channel's last sample × target, not gains (`tools/recomp-trace/send_vectors.py`).
- **Not retail in PR #4:** the 15 ms master delay and the 0.6·(FL + 0.707·C + 0.5·SL) stereo fold. The
  title's own output stage routes, interleaves and clamps to ±1, with no master gain.
- **Prior art:** `rwaudio-prior-work.md`:
  - nfsmw (CC0) and b5-decomp (reference only) name all 40 opcodes;
  - Iir2 filters are RBJ with Q = 1; the resampler is linear 16.16; de-click is 64 samples.
