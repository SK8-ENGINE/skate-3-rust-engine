# RenderWare Audio (rwaudiocore) + Snd9 AEMS: prior work for the native evaluator / voice-graph / mixer port

Research 2026-10-02. Knowledge only: everything below is written in our own words. Never paste code from these
repos into ours, whatever their licence (project rule "no copied game content": they transcribe EA code; we
re-implement). Companion to `aems-reference.md` (our ABKC/csi/opcode notes from PR #4 / sk8audio).

## Local copies (small sparse clones; delete freely, re-create with the commands below)
- BP-Decomp (4 MB): BurnoutDecomp/BP-Decomp_Workflow, sparse: `progress/audio_faithfulness/`,
  `progress/scratch_dossiers/*aems*|*sndplayer*`, `references/DecFIGS/dwarfdump/SDKs/{EATech,Packages}/…audio/csis…`,
  `references/DecFIGS/dwarfdump/GameShared/GameClasses/Sound/`, `tools/assets/bundles/aems_x360_port.py`.
- b5-decomp (5 MB): BurnoutDecomp/b5-decomp (branch `dev`), sparse:
  `vendor/renderware/{include,src}/rw/audio/`, `src/SDKs/EATech/include/{snd,NFSMix}/`,
  `src/GameShared/GameClasses/Sound/Playback/{AEMS,RWAC,Plugins}/`.
- nfsmw Snd9 (117 KB): 6 files fetched with `gh api … -H "Accept: application/vnd.github.raw"`
  from dbalatoni13/nfsmw `src/Speed/Indep/Libs/snd/9/` (aemsdef.h, saems.c, saemsi.h, saemsmbf.c, saemstimupdt.c, sndo.h).
- Sparse-checkout gotcha under Git Bash: leading-slash patterns get rewritten to the Git install folder; prefix
  the command with `MSYS_NO_PATHCONV=1`.
- The plug-in descriptor layout dossier in BurnoutDecomp/BP-Decomp_Workflow `progress/scratch_dossiers/` read via gh api only (not cloned).

## Sources, licence, applicability

| Source | What it is | Licence | Applies to Skate 3? |
|---|---|---|---|
| BP-Decomp_Workflow `references/DecFIGS/dwarfdump/SDKs/...` | DWARF outlines (no bodies) from an internal Burnout PS3 ELF: rwaudiocore **3.03.00**, csis **2.12.00**, Snd9 AEMS (`aemsdef.h`, `sndcmn.h`, `saemsi.h`). Enums, consts, struct field lists, GUIDs. | MIT (repo), but the content is EA's | High: same rwaudiocore line; FourCCs identical to ours |
| BP-Decomp_Workflow `progress/scratch_dossiers/*` | Machine-assisted decode dossiers of Burnout X360 ARTIST XEX: SndPlayer1 bodies (350 KB), AEMS/RWAC factory, plug-in descriptor rodata dumps | MIT | High for SndPlayer1/descriptors |
| BP-Decomp_Workflow `tools/assets/bundles/aems_x360_port.py` | X360→x64 ABKC bank porter; per-opcode block sizes + op-27 layout validation | MIT | High (same ABKC bytecode banks) |
| BP-Decomp_Workflow `IDA Files/ProStreet08Milestone.pdb` (referenced; 62 MB, not fetched) | Full PDB of NFS ProStreet X360 Oct-2007 (EA Black Box): authoritative rwaudiocore types | in repo, EA content | High (Black Box, same era); fetch only if a layout question is unanswerable otherwise |
| b5-decomp `vendor/renderware/src/rw/audio/core/*` | Reconstructed C++ of rwaudiocore: Iir2 + LowPass/HighPass/BandPass/Shelf/PeakingIir2, Gain, GainFader, Resample, Rechannel, Pan2D1, Pan2D, Send, SubMix, Mixer, MixKernels, Voice, SndPlayer1, Dac, Limiter1, ReverbModel1, Butterworth | **none** → reference only | High; but "PowerPC asm authoritative" claims are unverified, partially stubbed (Pan2D1 ComputeInteriorTerm = 0) |
| b5-decomp `src/SDKs/EATech/include/snd/sndaems.cpp` + `sndaemssampleplayer.cpp` | Full AEMS bytecode interpreter (all 40 opcodes) + AemsStandardSamplePlayer (player input → plug-in attributes) | none | Very high: same 40-op bytecode as Skate's ABKC |
| b5-decomp `src/SDKs/EATech/include/NFSMix/*` | NFSMixMap/NFSMixShape (EA "NFS mix" dynamic mixer; record layouts from the ProStreet PDB) | none | Medium: related lineage of Skate's MixMap (PR #1 already ports MixMapSK8.mxb at 99.999 %) |
| dbalatoni13/nfsmw `src/Speed/Indep/Libs/snd/9/` | **Matching** (1:1) decomp of NFS Most Wanted 2005 Snd9 incl. `saems.c` (AEMS interpreter) and DWARF-named `aemsdef.h` (every opcode's SETTINGS/STATE struct with offsets) | CC0 | High for names/semantics; older (2005, GC/PS2 gen); Skate's op-27 sample selection format differs |
| mitsevox/tw2004 `docs/reference-builds/tw07-ps3/cu/*.txt` | Per-CU DWARF function listings from Tiger Woods 07 PS3 (rwaudiocore **2.08.01**): function names, source line, PS3 address, size, locals, inlines. No bodies. Covers gain, gainfader, highpassiir2, lowpassiir2, iir2, resample, rechannelgainwrite, pan2d1, matrixpanner, send, smixer, sndplayer1, plugin, pluginregistry, voice, snd9, sndi_sin/cos | CC0 | Medium: names/local-variable hints only, older version |
| Kxboo/EA-Playground `_bevy/src/aems.rs`, `_bevy/docs/*` | Rust/Bevy reimplementation of EA Playground (Wii). ABKC there = **native PowerPC code** per class + fixups (not bytecode) | none | Low: confirms ABKC header field meanings; different flavour |
| Frostbite `LowPassIir2NodeData` hits (VU-Docs etc.) | Frostbite engine audio nodes: unrelated | n/a | No |

Version evidence: Skate 3 graph FourCCs (`SnP1 → Rch0 → Rsp0 → Gai0 → LI20 → Sen0`, `ems-emitters-re.md`)
are byte-identical to Burnout X360's registered descriptors (the plug-in descriptor layout dossier: AiW0 BI20 Dac0 Gai0 GaF0
HI20 HPB0 HS20 Li10 LI20 LPB0 LS20 Pn20 Pn21 Pau0 PI20 Rch0 Rsp0 RM10 Sen0 SnP1 Sub0). Skate 3's exact rwaudiocore
version is unknown (Burnout PS3 = 3.03.00, TW07 = 2.08.01, a Feb-2007 leak referenced by b5 = 2.11.00, not public).
AEMS timer formula reproduces our measured 6-block tick exactly (see 1 below) → same Snd9 AEMS runtime.

## Top facts (verify against Skate's XEX before relying on any of them)

### AEMS evaluator (b5 sndaems.cpp; names from nfsmw aemsdef.h)
1. Tick: timer callback per mixer frame (period δ = 256/48000 s default). Interval = max(1, ceil((1/rate)/δ) − 1)
   frames; step ms = interval·δ·1000. Burnout rate = 41.6 Hz → 4 frames; Skate's rate 30 → ceil(6.25)−1 = **6**
   frames = 32.0 ms: exactly our measured value. All instances on the update list run their program each tick.
2. Program record: u8 op, u8 assignment count, 2 pad, N × {s32 src, s32 dst}, s32 data advance; op 0xFF ends;
   op ≥ 40 aborts. src −1 = "the op result". Matches our grammar note.
3. Opcode names 0–39: ClassDestructor, ClassData, ClassDataTail / GlobalVariable (MW), Create, Destroy,
   CallFunction, Counter, Random, RandomShuffle, RandomWeighted, RangeTrigger, DelayTrigger, StateGenerator,
   Merge, Envelope, Table, DelayLine, **Mux (17)**, **Demux (18)**, **Minimum n-ary (19)**, **Maximum n-ary (20)**,
   Scale, Add, Subtract, Multiply, Divide, Modulo, Player (27), Oscillator, Ramp, AddMaximum (capped sum),
   SubtractMinimum, MultiplyMaximum, Minimum2, Maximum2, Scale2, Add2, Function (37), **ControlClass (38)**,
   SetGlobalVariable (39). → our "unknown/not ported" 19/20/38 are min/max of a u8-counted list and Csis class
   create/update/release (returns class ref count); our "stack top/push" for 17/18 are really Mux/Demux.
4. Semantics worth matching: Counter returns the input when inside [min,max], else steps & wraps when enabled;
   RangeTrigger = enter range fires 1 once, re-arms only inside a separate reset range; DelayTrigger counts ms by
   step, −1 idle; Envelope controls 1 start / 2 hold / 3 release (jump to release point), linear segments, value
   zero after last point; Table clamps, nearest when scale == 1.0 else linear between entries; DelayLine length =
   round(delay_ms/step) capped to capacity−1; Ramp delta = (target−cur)·step/duration/4096 × per-tick scale input.
5. Random: one global 6-word add-with-carry generator, fixed seeds F22D0E56 883126E9 C624DD2F 0702C49C 9E353F7D
   6FDF3B64 (same in Burnout X360 and XB1). Deterministic from boot; our "not reproducible run to run" is because
   other banks consume draws first, not because it's seeded from time (worth re-checking in Skate).
6. Rounding: b5 uses round-half-away; nfsmw (1:1 matching) uses SNDI_ftoifast and a 1024-step sine table
   (iSNDsin) for the oscillator; b5's std::sin is a host approximation. Prefer the table (our note: Skate table
   0x82FD36B8).
7. Player (op 27) block = nfsmw PLAYERSETTINGS: +0 bank, +4 sample group, +8 handle, +0xC prev control[2], +0xE input
   count, +0xF update-outputs flag, +0x10 sample type; state +0x14 sample select, +0x18 play control (0 stop, 1 play,
   2 pause). Outputs: timeleft/timecurr then up to 8 more. Inputs are 12-byte {selector, previous, value} records
   pushed only when changed. Skate/Burnout selection entry: u16 sample index (0xFFFF none), u8 level, 6 × azimuth
   bytes (<<8), s32 stream offset at +8 (MW's 12-byte SAMPLEENTRY {type, priority, union} is the older format).
8. ABKC header (DWARF names, nfsmw ModuleBank, 0x5C bytes): id, ver, veraimex major/minor/patch, **+8 platform
   (5 = Xenon)**, +0xA nummodules, +0xC debugcrc, +0x10 uniqueid, +0x14 totalsize, +0x18 residentsize,
   +0x1C moduleoffset, +0x20/+0x24 sfx bank offset/padded size, +0x28/+0x2C midi bank, +0x30 funcfixupoffset
   (native-code banks only; must be empty for bytecode), +0x34 staticdatafixupoffset, +0x38 interfaceOffset (Csis
   bindings: kind 0 global var, 1 class, else function), +0x3C.. runtime. → our "+0x08 variant 0x0503…" field
   should be re-read as platform byte + pad + nummodules.
9. Per-opcode data block sizes (aems_x360_port.py): 0→20, 3→4, 4→16, 6→24, 7→16, 10→24, 11→16, 15→16, 23/24/25→8,
   28→16, 29→28, 31→12, 33/34→8, 35→12, 36→8; op 27 = 28 + 12·inputs + (8 if updateoutputs) + (32 if outputs).

### Player inputs → voice graph (b5 sndaemssampleplayer.cpp, aemsdef PLAYER_INPUT_*)
10. Selectors: 0 PITCHMULT, 1 TIMEMULT (ignored), 2 VOL, 3 AZIMUTH, 4 ELEVATION (ignored), 5 FXWET0, 6 LOWPASS,
    7 HIGHPASS, 8 DRYLEVEL, 9–136 user. Clamps: 0/6/7 → 0..65535; 2/5/8 → 0..32767.
    Pitch → Resample attr 0 = v/4096. Vol/dry/wet = v·(1/32767) (constant 0.000030518509); Gain attr = vol·dry;
    Send attr = vol·wet. LPF/HPF attr = **raw value in Hz**. Azimuth → each Pan2D1 attr 0 =
    ((legacy table entry + v) & 0xFFFF)·180/32768 degrees, per channel voice (max 5 panner voices).
    Pause zeroes nothing in the snippet seen; Unpause re-applies gain/send/pitch.

### Voice graph plug-ins (b5 rwaudiocore reconstruction + DecFIGS DWARF)
11. Frame = MIXER_FRAME_SIZE 256 samples; every gain change de-clicks over GAIN_DECLICK_FRAME_SIZE 64 samples
    (linear from old to new over samples 0..63, then flat for 64..255). Same rule for Send and Pan2D1 ramps.
12. Biquad (Iir2, direct form I): y = b0x + b1x1 + b2x2 + 1e-18 − a1y1 − a2y2; coefficient order {a1,a2,b0,b1,b2}.
    LowPassIir2/HighPassIir2 are RBJ cookbook with **Q = 1** (alpha = sin(ω)/2), ω = 2π·fc/fs, sin rounded to f32.
    LPF: bypass + state clear when ω ≥ 3.1384511 (≈ 23.98 kHz at 48 k → 25000 = open), ω floored at 0.0031416.
    HPF: bypass when ω ≤ 0.0031416 (fc ≈ 24 Hz; default 0 = off), ω capped at 3.1384511. Coefficients recomputed
    only when ω changes. Both set decay 450 samples.
13. Resample: 2-tap linear interpolation, 16.16 fixed-point phase, increment = round(ratio·65536), ratio =
    (sourceRate/outputRate)·pitch clamped to 4× (MAX_RESAMPLE_RATIO). Fraction scale constant is 1.5258e-5
    (0x377FFC9C), not exactly 1/65536. 6 floats history per channel. PreProcess multiplies a context
    "pitch/rate" accumulator by the clamped ratio (b5 calls it a gain; DWARF Mixer has mTotalPitch/ScalePitch,
    probably that). On a source-rate change, the frame passes through unprocessed once.
14. Rechannel: only acts when in ≠ out channels. Kernel table: mono→5.1 = centre only; stereo→5.1 = FL/FR;
    5.1→stereo L = 0.707·C + FL + BL (LFE dropped); quad→stereo L = FL+BL. rwaudio 5.1 order:
    **0 FL, 1 C, 2 FR, 3 BL, 4 BR, 5 LFE** (DecFIGS channel.h). MAX_CHANNELS 6.
15. Pan2D1 (GUID 'Pn21', 7 attributes: azimuth°, radius, width, spread°, focus, level, centre level; ctor params
    front angle 45°, rear 135°, normalisation mode 2 = 1/sqrt(N emitters)). Speakers: dir0 +45 (FL), dir1 0° (C),
    dir2 −45 (FR), dir3 +135 (BL), dir4 −135 (BR); azimuth is negated (positive degrees pans right). Pairwise
    inverse 2×2 matrices (VBAP style), per-source 5-vector normalised to unit power × norm × level; stereo
    out = constant-power pair from target y. 6-ch input → 5 emitters + LFE copy at centre-level gain.
    **b5's ComputeInteriorTerm is a stub**: the radius<1/spread interior term is still unknown.
16. Send: accumulates (MixWithGainRamp) into a SubMix; gain attr 0, de-click ramp; connect by pointer or by SubMix
    name. Gain: attr 0 linear, init 1.0.
17. Plug-in types: 0 INPUT_SOURCE (SndPlayer1), 1 INPUT_RECHANNEL, 2 INPUT_RESAMPLE, 3 INPUT_PROCESS, 4 STANDARD.
    Descriptor 52 bytes: name, GetSize, CreateInstance, PreProcess, Process, channel maps, param desc, event desc,
    tool desc, list node, guid, type, #ctor params, #attributes, #events, variable in/out flags, registry index.
    Only Resample, Rechannel and SndPlayer1 have a live PreProcess (pull-model sample count negotiation).
18. Voice: ExpelReason {not, error, cpu limit, playback over}; decay samples accumulate per stage (filters add 450)
    so a released voice keeps running for the filters' tail before expel. PRIORITY_PERMANENT exists.
19. SndPlayer1: header bitfields version 4, codec 4, channels 6, sample rate 18, play type 2, loop flag 1,
    num samples 29, loop start 32; chunk header {bytes, samples}; PlayParams {startTime f64, streamFileOffset f64,
    path, pRamData, streamPoolGuid, expelMode, requestHandle}; MAX_DECODERFEEDS 20; declick on start/stop.
    350 KB of decoded bodies in `sndplayer1_bodies_decode.md` if we need exact start/loop/stream behaviour.
20. Util constants exist for dB/cents conversion (DECIBEL_MIN, CENTS_MIN, PITCHLINEARTOCENTSCONST) but values are
    not in the DWARF; NFSMixShape gives 2^(cents/1200) pitch and Q15↔hundredths-dB tables (MixMap side).

## Uncertainties
- b5-decomp is an unverified reconstruction ("asm authoritative" but unverified by us); parts are host adaptations
  (x64/XB1-guided), some bodies stubbed. nfsmw is a byte-matching decomp but 2005-era. Always confirm against
  Skate's XEX (recomp build) before treating a detail as retail truth (project rule: retail parity).
- Skate 3 rwaudiocore version unknown; possible small differences (e.g. Resample kernel taps, Pan2D1 interior term).
- MixMap: Skate uses "PathFinder 5.03 MixMap" (PR #1); NFSMixMap is a sibling, not proven identical.
- ProStreet PDB not fetched (62 MB). Fetch only for a specific unanswered layout question.

## Recommendations for our port (summary)
- Evaluator: adopt the 40 official op names and nfsmw state-struct field names as our Rust names; implement
  19/20 (n-ary min/max), 17/18 (mux/demux) and 38 (control class → our Csis bridge) per the semantics above;
  the timer formula with rate 30; the 6-word global RNG with the fixed seeds (one shared generator across all
  instances, drawn in list order).
- Voice graph: implement stages as 256-sample block processors with 64-sample linear de-click on every gain change;
  RBJ Q=1 biquads with the exact bypass thresholds; 16.16 linear resampler with the 0x377FFC9C fraction constant
  and 4× clamp; rwaudio 5.1 channel order internally, reorder only at the device.
- Pan2D1: port SpeakerConfig/EmitterConfig/ComputeLevels/Ramp as described; measure the interior term from the
  Skate XEX (b5 lacks it); for mono point sources at radius 1 the perimeter path alone may suffice.
- Validate each stage with short scripted recomp runs, comparing per-block output.
