# AEMS patch-program evaluator: behavioural specification (Skate 3 TU3)

Basis for a native Rust re-implementation. Written 2026-10-02 in our own words. Facts (offsets, opcode
numbers, constants, addresses) come from:
- the TU3 (title update 3) recomp (`skate3recomp/generated`, read with `tools/recomp-code-search/fn.sh`);
- the disc banks (our survey script `tools/audio-file-inspect/aems_survey.py`, all 376 `.abk` and 9 `.csi`);
- the PoC probe (`emitter_probe` in our local PoC worktree, which runs upstream PR #4);
- PR #4 and sk8Audio (reference only, no licence);
- nfsmw Snd9 (dbalatoni13/nfsmw, CC0; see `rwaudio-prior-work.md`). It is the source of the official names, and it is
  2005-era, so differences are marked.

Licence rule: nothing here is code from PR #4, the recomp or b5-decomp. Describe, don't transliterate.

Status tags: **[V]** verified against retail code or data by us; **[P]** taken from PR #4's reading of
retail code and spot-checked; **[N]** from nfsmw 2005, consistent with Skate but not re-read in TU3;
**[?]** open.

Related notes: `aems-reference.md` (older summary; corrected by §11 here), `ems-emitters-re.md` (game side
of emitters), `rwaudio-prior-work.md` (prior art, voice graph).

---

## 0. Vocabulary (official AEMS/Csis names → what we called them before)

| Official (nfsmw/Csis) | Earlier notes / PR #4 | What it is |
|---|---|---|
| ModuleBank | `.abk` / ABKC bank | a compiled bank: modules + sample bank |
| Module | "input record" | one patch: program, instance template, capacity, bound to one Csis class |
| ModuleInstance | instance | a live copy of the module's template; owns voices |
| Csis project | `.csi` / MOIR | the authoring project's symbol table |
| Csis **Class** (csi table 1) | "object" (`c_emitter`, `Class_grind`) | what game code posts to |
| Csis **Function** (csi table 0) | "message/broadcast" (`*_msg`) | a fire-and-forget call with parameters |
| Csis **GlobalVariable** (csi table 2) | "variable/global" (`*_snd`, `*_vol`, `*_gbl`) | an int with subscribers |
| class instance / "pClass" | post node | the 16-byte object a post creates; refcounted |
| member data | payload words | the int parameters of a class instance |
| ClassDestructor client | release callback | told when the poster releases |
| timer client list | evaluator node list (`0x83036F4C`) | the instances the tick runs |
| Player | voice object (op 27) | plays one sample from a sample group |
| SampleGroup / sample entry | descriptor table / descriptor | the per-player list of selectable samples |
| TABLE | curve | sampled lookup table used by op 15 / op 9 |
| tick scale ("gVariableTimerPeriod") | `0x830775D8` | the tick length in ms (≈32) |

---

## 1. Containers

All multi-byte fields are big-endian. Offsets are from the start of the file (on disk). After load, every
offset listed in the rebase list becomes an absolute pointer. For a native port, keep offsets and resolve
on access.

### 1.1 ModuleBank (`.abk`, magic `ABKC`): header, 0x5C bytes [V]

| Off | Name | Disc value / meaning |
|---|---|---|
| 0x00 | id | `ABKC` |
| 0x04 | ver, aimex major/minor/patch | bytes 01 01 02 02 on all 376 |
| 0x08 | platform | 5 = Xenon (all) |
| 0x09 | (pad) | 3 on all banks (meaning unknown) |
| 0x0A | u16 nummodules | 1 (369 banks), 2 (5), 3 (2): 385 modules total |
| 0x0C / 0x10 | debugcrc / uniqueid | 0 |
| 0x14 | totalsize | = file length |
| 0x18 | residentsize | = sample-bank offset (modules occupy 0x5C..this) |
| 0x1C | moduleoffset | 0x5C, the first Module |
| 0x20 / 0x24 | sfx bank offset / padded size | the `S10A` sample bank |
| 0x28 / 0x2C | midi bank | 0 |
| 0x30 | funcfixupoffset | list {count, offsets}; count 0 on every bank (native-code banks only) |
| 0x34 | staticdatafixupoffset | rebase list {u32 count, u32 offset[count]}: each offset names a word in the bank that holds a bank-relative offset (4,656 words over all banks) |
| 0x38 | interfaceOffset | interface (export) list, see 1.2 |
| 0x3C..0x5B | run time | bank id (+0x3C), sample-bank pointer (+0x40, `FFFFFFFF` on disk), midi (+0x44), stream file path (+0x48), stream file offset (+0x4C), bank list node (+0x50/+0x54), tweak header (+0x58) |

**Sample bank `S10A`** [V]: `'S10A'`, u32 0, u32 capacity, then u32 offsets[capacity] relative to the
tag. Used slots form a prefix, and `FFFFFFFF` marks an unused slot. Each used slot is an EA Audio Core
stream header plus data: mostly XMA, mono, 48 kHz. A loop flag and loop start are in the header (see
`gotchas`/PR #4 docs: loop headers are 12+ bytes).

### 1.2 Interface references (exports) [V]

`u32 count`, then count × 12 B: {u32 handle offset (where the resolved handle goes), u32 ID-record offset,
u32 kind}. The ID record is {u16 project id, u16 name id, NUL-terminated name, padded to 4}.

**kind top byte = InterfaceType:**
- **0 GlobalVariable** → csi table 2 (362 exports: `*_snd`, `*_gbl`, `*_vol`, …);
- **1 Class** → csi table 1 (386: `c_*`, `Class_*`);
- **2 Function** → csi table 0 (311: `*_msg`, …).

The low 24 bits take 5 values whose meaning is unknown and unused at run time. A resolved handle is
8 bytes: {symbol record pointer, u32 (name_id << 16 | generation)} (§1.4).

### 1.3 Module (the old "input record"), 60 bytes + 4 per extra [V]

| Off | Field | Meaning |
|---|---|---|
| +0x00 | id | unused (0) |
| +0x04 | classHandle (8 B) | the Csis class this module serves (its export target) |
| +0x0C | constructorClient (16 B) | {next, prev, fn, ctx} list node hung on the class's constructor list at install |
| +0x1C | s16 curinstances | live count (0 on disk) |
| +0x1E | s16 maxinstances | **capacity**: 10 (300 modules), 1 (30), 16 (28), 3 (13), 5 (5), 4 (3), 2, 6, 8, 20, 24 |
| +0x20 | u16 numGlobals | GlobalVariable subscriptions in the template |
| +0x22 | u16 numFunctions | Function subscriptions in the template |
| +0x24 | u8 numPlayers | Player (op 27) states |
| +0x25 | u8 classDestructorPresent | template has a ClassDestructor state |
| +0x26 | u8 classDataPresent | template has a ClassData (payload) state |
| +0x27 | u8 numClassControllers | ControlClass (op 38) states |
| +0x28 | pcode | program offset |
| +0x2C | pdata | instance template offset |
| +0x30 | datasize | template/instance size |
| +0x34 | destroydataoffset | offset of the Destroy (op 4) state in the instance; = datasize − 16 on all 385 |
| +0x38 | moduleInstance | live-instance list head (run time) |
| +0x3C | u32[numPlayers + numClassControllers] | template offsets of each Player state, then each ControlClass state |

The next module follows immediately. Install writes the bank pointer into +0 of every Player state (the
first numPlayers offsets).

### 1.4 Instance template (pdata) layout [V on all 385]

| Instance offset | Content |
|---|---|
| 0..7 | live-list links {next, prev} |
| 8..23 | timer client {next, prev, program, data pointer = instance+24} |
| 24.. | ClassDestructorState, 20 B, if present: client(16) + **triggered** (+16) |
| then | numGlobals × GlobalVariableState, 28 B: handle(8) + client(16) + **value** (+24) |
| then | ClassDataState, if present: client(16) + u8 numOutputs (+16) + **values[numOutputs]** (+20). Size (n+5)·4 |
| then | numFunctions × FunctionState: handle(8) + client(16) + u8 numParameters (+24) + u8 **triggered** (+25) + **values[n]** (+28). Size (n+7)·4 |
| then … | the op data blocks, in program order |
| datasize−16 | Destroy state {module, instance, class instance, triggered} = the last op's block |

**Block walk invariant [V]:**
- the program's data pointer starts at instance+24;
- each op record advances it by its own amount, always ≥ 0 on disc;
- it ends exactly at datasize on all 385.

Ops 0/2/1/37 read the subscription states above, so their blocks *are* those states.

**Payload word counts (numOutputs) on disc:** 9 for 296 modules (all `c_emitter`); also 0, 7, 8, 10–13,
15, 17, 18, 20–22, 25, 27, 28, 34, 50, 54.

### 1.5 Program encoding [V]

A sequence of records, ended by opcode 255:
`u8 opcode, u8 npairs, u16 unused (0 on all 31,016), npairs × {s32 src, s32 dst}, s32 advance`.

All opcodes on disc are < 40. Census (ops per opcode, all banks):

0:385 · 1:385 · 2:252 · 3:385 · 4:385 · 5:182 · 6:50 · 7:630 · 8:599 · 9:3 · 10:2545 · 11:1186 · 12:1902 ·
13:837 · 14:12 · 15:3126 · 16:190 · 17:2653 · 18:171 · **19:0** · **20:42** (arena_cheers, arena_ohhs,
arena_negative_reax, car_alarms) · 21:366 · 22:51 · 23:969 · 24:863 · 25:709 · 26:267 · 27:1527 · 28:372 ·
29:800 · 30:1958 · 31:1450 · 32:49 · 33:310 · 34:1145 · 35:2093 · 36:1927 · 37:129 · **38:1** (Tazer.abk,
c_tazer) · 39:110.

Typical frame:
- programs start with 0, then 1/2, then 3 (ClassDestructor, ClassData/GlobalVariable reads, Create);
- they end with `…, 4` (Destroy), most often preceded by 27 (217), 5 (77), 10, 36 or 12.

### 1.6 Player state (op 27 block) [V layout; P semantics]

| Off | Field |
|---|---|
| +0 | bank pointer (written at install) |
| +4 | sample group pointer (rebased) |
| +8 | voice handle (0 = none) |
| +12 | s8 prevplaycontrol[0] (last applied control) |
| +13 | s8 prevplaycontrol[1] (the one before) |
| +14 | u8 numinputs (7 typical; up to 11+) |
| +15 | u8 updateoutputs (1 on 819 of 1527 players): write timeleft/timecurr |
| +16 | s8 sampletype: `FF` on disc; unused by Skate's player code [?] |
| +17 | u8 extra outputs: 8 more output words after the time pair (0 on all disc players) |
| +20 | **sampleselect** (input) |
| +24 | **playcontrol** (input): 0 stop, 1 play, 2 pause |
| +28 | numinputs × 12 B input states {u8 type, pad, s32 applied (FFFFFFFF on disc), s32 value} |
| after inputs | if updateoutputs: **timeleft** (ms), **timecurr** (ms); then 8 words if +17 |

**Sample group** (Skate's newer format, differs from 2005's typed SAMPLEENTRY) [V]: u32 count, then
count × 12 B:
- s16 sample index into S10A (`FFFF` = silent entry);
- u8 level % (×0.01 → the voice's player level; 40–100 seen);
- 6 bytes of per-channel azimuths, each <<8 into 65536 = 360° units. Mono uses the first. The later bytes
  look like `{…, slot+1, FF}` on many banks, meaning unknown [?];
- s32 stream offset at +8. `FFFFFFFF` = in-memory sample; it is added to the bank's stream file offset.

Counts run from 1 up to 10001 (Sk8_Air_Flip_Tricks, equal to its S10A capacity).

**Input type ids (PLAYER_INPUT_*) and use on disc:**

| id | Name | Effect | Count |
|---|---|---|---|
| 0 | PITCHMULT | 1525 | resample ratio = value/4096 (clamp 0..65535) |
| 1 | TIMEMULT | n/a | ignored |
| 2 | VOL | 1527 | master level value/32767 (clamp 0..32767); multiplies dry and send |
| 3 | AZIMUTH | 1516 | pan angle = value·360/65536 degrees (alternate setter) |
| 4 | ELEVATION | 2 | ignored |
| 5 | FXWET0 | 1515 | send level/32767 × master (clamp 0..32767) |
| 6 | LOWPASS | 1527 | low-pass cutoff, raw Hz (clamp 0..65535; 25000 = open) |
| 7 | HIGHPASS | 1521 | high-pass cutoff, raw Hz (clamp 0..65535; 0 = off) |
| 8 | DRYLEVEL | 1513 | dry gain/32767 × master (clamp 0..32767) |
| 9..136 | user / routing | 156 (9), 29 (10), 536 (11), 29 (12) | read at open as routing codes (§5.4); id 11 also = effect-send level/32767 when an effect bus is routed; others ignored afterwards |

### 1.7 TABLE (op 15 curve; op 9 weights) [V]

| Off | Field |
|---|---|
| +0 | u8 entry size: 1 = s8, 2 = s16, anything else s32 |
| +2 | u16 count |
| +4 | s32 min input |
| +8 | s32 max input |
| +12 | f32 resolution: entries per input unit; exactly 1.0 = nearest |
| +16 | entries |

On disc:
- s8 1917 (311 nearest), s16 1127 (609 nearest), s32 82 (all nearest);
- the op-15 "previous input" starts at `7FFFFFF1` (forces a first evaluation).

### 1.8 Csis project (`.csi`, magic `MOIR`) [V]

| Off | Field |
|---|---|
| 0x00 | `MOIR` |
| 0x04 | u16 3, u16 0x0100 |
| 0x08 | u8 5, u8 0 |
| 0x0A | u16 #functions (table 0) |
| 0x0C | u16 #classes (table 1) |
| 0x0E | u16 #globals (table 2) |
| 0x10 | u16 project id |
| 0x14 / 0x18 / 0x1C | run-time table pointers |
| 0x20 | project list node |
| 0x28 | tables 0, 1, 2 back to back, then the string pool |

**Records:**
- Function / Class, 12 B: {+0 client-list head (0 on disc), +4 name offset, +8 u16 name id,
  +10 u16 generation (0 on disc)}.
- GlobalVariable, 16 B: {+0 client-list head, **+4 s32 value = default**, +8 name, +12 u16 name id,
  +14 u16 generation}.

Name ids ascend within a table. The 9 projects:

| File | Project | functions / classes / globals |
|---|---|---|
| SK8_AEMS_Foley | 0x5C48 | 27 / 10 / 161 |
| Sk8_Emitters_Project | 0x2E5A | 136 / 11 / 144 |
| SK8_AEMS_skateboard | 0x64BD | 3 / 54 / 24 |
| SK8_AEMS_rolling | 0x4EA6 | 5 / 1 / 12 |
| SK8_AEMS_Crowds | 0x167A | 3 / 3 / 9 |
| Sk8_AEMS_MoveableObjects | 0x69E3 | 1 / 7 / 34 |
| AEMS_TRAFFIC | 0x2DED | 4 / 3 / 0 |
| Sk8_moments | 0x796A | 0 / 8 / 1 |
| AEMS_Calibrate | 0x0F52 | 0 / 1 / 0 |

**Install** [P]:
1. Convert name offsets to pointers.
2. Give every record a fresh generation from one global u16 counter. It pre-increments per record and
   restarts at 1 when it would turn negative as an s16.
3. Link the project into the project list (newest first).

**Lookup** (`sub_828E3148/3250/3358` for tables 0/1/2) [P]:
1. Pass 1 scans only projects whose id matches the query's project id. Pass 2 scans every project.
2. A hit needs an equal name id **and** an equal name string. It writes {record, record's id|gen word}
   into the handle.
3. A miss returns −5.

Pass 2 is why banks built against unshipped projects (0x63D9, 0x4228, …) still bind. 4 of 1,059 exports
find nothing: `semi_horns_msg`, `pa_announce_a_glb` ×2, `pa_announce_b_glb`.

**Handle check (every use)** [V]:
- handle word negative → return it as the error;
- record pointer null → −6;
- word ≠ the record's current id|gen → clear the handle (record 0, word −3) and return −3.

### 1.9 SPLC `.bnk`

Not part of AEMS. These are Splice sample collections, played by game code directly (SFXObj_Contacts,
the collision manager). They are not bound to any csi and have no program. Field correction (member +4 =
gain, +8 = pitch): `aems-reference.md`, CORRECTION section.

### 1.10 How a bank binds to `Sk8_Emitters_Project.csi:c_emitter` (worked example) [V]

1. Load all csi first, then each bank. Banks are installed only after their csi projects.
2. Resolve each export into its handle slot inside the bank:
   - the module's classHandle (kind 1 → class `c_emitter`);
   - GlobalVariable handles in the template (kind 0, e.g. `siren_city_snd_1`);
   - Function handles (kind 2, e.g. `siren_city_msg_1` in an op-5 block).
3. If the module's classHandle is valid, push its constructorClient (fn = "create instance",
   ctx = module) onto the class record's client list. Newest bank first.
4. 291 banks bind to `c_emitter`. A post to `c_emitter` reaches **every loaded** bank bound to it, and
   each spawns an instance (§2.3).
5. The payload's selector word (w8 = the attribute patch index) decides inside each program whether that
   bank plays; non-matching instances stay silent until released (`ems-emitters-re.md`).

---

## 2. Runtime model

### 2.1 Objects and state

- **Global evaluator state:**
  - instance list (a stack: new instances are pushed at the head);
  - tick-period cache {cached delta f32, period count u32, countdown u32};
  - tick scale f32;
  - the RNG (§2.8);
  - the global u16 generation counter;
  - the project list.
- **Class instance** (post node), 16 B: {class record, refcount, ClassData client list, ClassDestructor
  client list}.
- **Module instance:** a byte copy of the template (§1.4) with live lists linked. All op state lives in
  it. No other per-instance state exists, except voice handles and the post-node pointer.

### 2.2 Posting (CreateInstance; `sub_828E2B48`) [V]

Inputs: a class handle, a pointer to the payload ints, and an out-slot for the post node.
1. Validate the handle (§1.8). On failure return the code: −3/−5/−6, or −1 when out of memory.
2. Allocate the post node: refcount 1 (the poster's reference), empty lists.
3. **Call every constructor client** of the class, newest bank first. Each runs "create module instance"
   with (post node, payload, module).
4. Then **call every ClassData client** now on the node; each copies payload words into its instance
   (§2.4).
5. Store the node in the out-slot and return 0.

**The post still succeeds** when no instance was created (capacity full, no bank loaded).

### 2.3 Create module instance (`sub_82B1DAD0` → `sub_82B1D880`) [V/P]

- If curinstances ≥ maxinstances, do nothing (silently). Otherwise:
  1. Allocate datasize bytes and copy the template.
  2. Fill the Destroy state's {module, instance, post node}.
  3. Link into the module's live list (head) and set the timer client {program, data = instance+24}.
  4. ClassDestructor present → register its client on the node's destructor list; node refcount +1.
  5. For each GlobalVariable state: subscribe its client to the global; **copy the global's current
     value into +24 immediately**.
  6. ClassData present → register on the node's ClassData list; refcount +1. Values are filled by step 4
     of §2.2, or by a later redelivery.
  7. For each Function state: if its handle is valid, subscribe to the function (stale → handle cleared).
  8. curinstances +1.
  9. **Push the timer client at the head of the evaluator list.** A new instance runs first in the next
     walk.
- Nothing runs at creation; the first program run is at the next walk (§2.6).

### 2.4 Payload, held messages and redelivery [V]

- ClassData copy: numOutputs words (bank-defined) from the payload into values[]. **The count comes from
  the bank, not the poster**: the poster must supply at least that many words.
- **Redelivery (`sub_828E2D18`, "SetMemberData"):** the game rewrites its payload and calls this with
  (node, payload). It reruns only the node's ClassData clients. Constructors are not rerun, so no new
  instances.
- Game components keep a node ("held message") and redeliver every 60 Hz game frame. The evaluator only
  sees the last value before each walk (31.25 Hz).
  - A "one-shot" use = post, then release soon after. Whether the sound survives depends on the program
    (§2.5).
  - c_emitter programs end at once on release, so emitters must be held for as long as they sound.
- Example senders: `Class_grind` (sub_824AF8C8 post, sub_824C39E0 per-frame redelivery);
  `SFXObj_Emitter` (sub_824DCE60 create, sub_824DCF08 per frame; see `ems-emitters-re.md`).

### 2.5 Release, destroy, lifetime [V/P]

**Release (`sub_828E2C78`):** under the audio lock:
1. call every destructor client on the node; each sets its instance's ClassDestructor.triggered = 1;
2. drop the poster's reference, and free the node at 0.

**Program side:**
- Op 0 reads and clears `triggered`. Programs route it, directly or through logic, into the Destroy
  state's triggered word (+12).
- Op 4 (Destroy), the last op, removes the instance in the **same walk** when that word ≠ 0:
  1. unlink it from the module live list and from the evaluator list;
  2. unregister its destructor client and its ClassData client, each dropping a node reference (free at 0);
  3. unsubscribe its GlobalVariable and Function clients;
  4. for each Player with a voice, **release the voice** (immediate stop at the AEMS level; any de-click
     is the voice graph's business);
  5. for each ControlClass with a node, **release that child post** (cascades to the child's instances);
  6. curinstances −1, free.
- The walk is safe: the interpreter fetched the next instance before running this one.

**Lifetimes:**
- c_emitter banks: Destroy.triggered = the ClassDestructor pulse. The instance lives exactly until
  release; finished voices do not end it.
- Some programs compute their own end (e.g. after their voices finish). Then the instance ends while the
  game still holds the node. That is harmless: later redeliveries find no clients, and the release just
  frees the node.

### 2.6 Timing: the tick [V]

The tick function (`sub_82B1E290`) is called once per **audio block of 256 frames at 48 kHz**, with the
block delta δ as an **f32** (scheduler +64).

- **Period (re)computation** whenever δ differs bit-for-bit from the cached f32:
  1. limit = f32(1.0 / D). D = 30.0 in play: game init `sub_826D4C30` copies it from 0x820D4924 over the
     boot value 41.6.
  2. Accumulate f32 sums: count = the number of whole δ steps taken until one more step would reach the
     limit. That is count = ceil(limit/δ) − 1 for non-exact cases, at least 1.
  3. Store count as both the period and the countdown.
  4. tick scale = f32( f32(f32(count) · δ) · 1000 ).
- **Every call:** countdown −= 1; at 0, reload it with the period and **walk**.
- With δ = f32(256/48000) = 0.0053333333:
  - count = **6** → a walk every 6 blocks = 31.25 Hz;
  - tick scale = **31.999998 ms** (not exactly 32: use the f32 value).
  - On the first call after a recount the countdown becomes count−1, so the first walk is on the 6th
    call.
- **Gotcha [V]:** feeding an f64 δ that is not f32-exact makes every call a "changed δ" → the countdown
  never reaches 0 → nothing runs. The native port should simply keep a block counter and walk every 6th
  block, with tick scale 31.999998.
- **Walk:** visit each instance from the head of the list (newest first) and run its program over its
  data (§3). A post or redelivery made between walks takes effect at the next walk. The phase between a
  post and the first walk is 0–5 blocks (0–27 ms).

### 2.7 Functions and globals (the `*_msg` → `*_snd` machinery) [V]

**CallFunction (op 5)** delivers its parameters **synchronously** to every subscriber of the function.
- Each subscriber's FunctionState copies numParameters words (its own count) and sets triggered = 1.
- The subscriber sees it when its own program next runs op 37:
  - later in the **same walk** if it is older (further down the list);
  - otherwise in the next walk.
- Game code can call functions too (same entry point `sub_828E29C0`).
- Result codes: −4 (no subscriber), −3, −6. All are ignored by op 5.

**SetGlobalVariable (op 39)** clamps and stores into the global record's value. The value is clamped to
the block's [min, max]; the raw value is kept as "prev".
- **Only if the stored value changes**, every subscriber is notified, and each copies it into its +24
  synchronously.
- New subscribers copy the current value at creation (§2.3).
- Globals start at the csi default (+4).
- Game code can set globals too [?: the "G" writer `*(0x83083C38)+0x2FCB4` in PR #1 notes].

**Emitter project round trip:**
- `c_emitter_utility` is posted once at boot and held. Capacity 1; 97 Function subscriptions; 100 op 39.
- For each `*_msg` function:
  1. op 37 (did someone call it?) gates an op 8 shuffle (triggered input);
  2. the shuffle draws the next sample number;
  3. op 39 publishes it into the paired `*_snd` global.
- Sound banks read `*_snd` through a GlobalVariable state (op 2) into a Player's sampleselect.
- Their Create pulse (op 3) triggers op 5 on `*_msg` to request the next number.
- Because the requester is newer than the utility (it runs first in the walk):
  - **a post's first player opens with the number published by the *previous* request;**
  - the new number arrives in the same walk, after the requester ran;
  - a second layer opened ticks later (e.g. Siren_city_1 player 2) reads the *new* value [? confirm
    with golden/retail].

### 2.8 Random numbers [V]

One global generator, 6 u32 words at 0x830775F0 (call them w0..w5). It is shared by every instance and
used only by ops 7, 8 and 9 (no other caller in the recomp).

**One draw:**
1. Ripple-add upward with carry:
   - w4 += w5;
   - w3 += w4' + carry; w2 += w3' + c; w1 += w2' + c; w0 += w1' + c.
   - Each step uses the freshly updated lower word. Its carry is "result low word < original low word".
     The first step also checks against w5.
2. Then w5 += 1. If w5 wrapped to 0, add 1 to w4, w3, w2, w1, w0 in turn, stopping at the first word
   that does not wrap to 0.
3. The value used by every op is the **low 32 bits of the new w0** (ops take `draw mod n` on that
   unsigned word). After a full ripple it is w0+1.

**Seed:**
- The boot image holds **all zeros** at 0x830775F0, and no TU3 code writes these words except the draw
  itself.
- The fixed constants F22D0E56 883126E9 C624DD2F 0702C49C 9E353F7D 6FDF3B64 sit unreferenced at
  0x82FD36A0, right after the opcode table.
- Game init `sub_826D4C30` seeds a **different** 6-word generator (0x82FD7D74, used by `sub_82A8AF10`)
  with (time base + those constants).
- So AEMS draws are deterministic from zero but depend on every draw since boot. That is why the results
  are not reproducible across runs.
- [?] Confirm with a passive recomp read of the 6 words at the first post.
- **Port:** start at zero. Expose seeding for tests; golden runs must log the state.

---

## 3. The interpreter (per instance, per walk) [V]

Data pointer `d` = instance+24; record pointer = program start.

While opcode ≠ 255:
1. r = op(opcode)(block at `d`). Each op returns a 32-bit value; only the low word is ever stored.
2. For each pair, in order (npairs is re-read every iteration, but no op writes programs, so it is
   constant):
   - src = −1 → store r at d+dst;
   - else copy the word at d+src to d+dst.
   - Offsets are bytes, relative to the **current** block, and may be negative (into earlier blocks).
3. d += advance; move to the next record.

**Data flow:** an op's inputs are words in its own block that earlier ops' pairs wrote. Values persist
across walks, so an op's input keeps the last written value until rewritten. Result fan-out happens only
through pairs.

**Opcodes ≥ 40 do not occur.** The port should treat them as a hard error at bank load.

**Integer semantics:** treat every word as i32 with wrapping arithmetic. Results are stored as the low
32 bits. Retail keeps 64-bit intermediates in some ops, but every store and every comparison it makes
uses the low word, so 32-bit wrapping is exact. Exceptions (op 9 unsigned compare) are noted per op.

**Float semantics:** all float math is **f32** with round-to-nearest after each operation:
- int → float conversion = exact to f64, then round to f32;
- "fused" means one rounding of a·b+c (PPC fmadds): use an f64 FMA then round to f32; double rounding is
  theoretically possible but immaterial at these magnitudes [?];
- **round(x)** = (x < 0 ? x − 0.5 : x + 0.5) in f32, then truncate toward zero to i32. Saturate like PPC
  fctiwz: >2³¹−1 → 0x7FFFFFFF; < −2³¹ or NaN → 0x80000000. Rust `as i32` maps NaN to 0, so special-case
  it.

---

## 4. Opcodes

Block offsets are from the op's block. "trig" = a trigger input word, where non-zero means true. All ops
return i32.

| # | Name | Block layout | Behaviour | PR #4 |
|---|---|---|---|---|
| 0 | ClassDestructor | the ClassDestructorState: +16 triggered | return triggered, then clear it (one-shot "released" pulse) | ported |
| 1 | ClassData | ClassDataState: +16 u8 n, +20 values | return values[0]; programs fan the other payload words out with copy pairs | ported |
| 2 | GlobalVariable | GlobalVariableState: +24 value | return value (no clear) | ported |
| 3 | Create | +0 trig (1 in every template) | return trig, then clear it → 1 on the first walk only | ported |
| 4 | Destroy | +0 module, +4 instance, +8 post node, +12 trig | trig ≠ 0 → destroy the instance (§2.5); return 0 | host op |
| 5 | CallFunction | +0 function handle (8 B), +8 u8 clamp flag, +9 u8 n, +12 [n × {min,max} if flag], then inputs {trig, params[n]} | if flag: clamp each param in place to its range (always, triggered or not); if trig ≠ 0: call the function with params (§2.7). Trig is not cleared. Return 0 | **host op, wrong handle (bug, §11)** |
| 6 | Counter | +0 min, +4 max, +8 value, +12 s8 step, +16 trig, +20 override | min ≤ override ≤ max → return override, no state change. Else if trig > 0 (signed): value += step; if value > max → min; else if < min → max. Return value | ported |
| 7 | Random | +0 min, +4 range, +8 current, +12 trig | trig ≠ 0 → current = min + (draw mod range), unsigned; range 0 → min + draw. Return current | ported |
| 8 | RandomShuffle | +0 u16 trig offset (self-relative), +2 u8 entry size (1 = u8, else u16), +3 s8 avoidrepeat, +4 min, +8 u16 index, +10 u16 range, +12 current, +16 number set | trig == 0 → return current. Else span = range − avoidrepeat − index; k = index + (draw mod span); output = set[k]; swap set[k] ↔ set[index]; current = output + min; index += 1; if index ≥ range → index = 0, avoidrepeat = 1; else avoidrepeat = 0. Return current. The last pick of a pass is excluded from the next pass's first pick. Span 0 is undefined (never on disc) | ported |
| 9 | RandomWeighted | +0 TABLE ptr (weights = its s8 entries), +4 min, +8 count, +12 current, +16 trig | trig ≠ 0 → r = draw mod 100; sum the signed weights in order and take the first i where (sum as u32) > r: current = min + i. If none, unchanged. Return current. A negative sum compares as huge and fires at once | ported |
| 10 | RangeTrigger | +0/+4 trip [lo,hi], +8/+12 reset [lo,hi], +16 s8 tripped, +17 s8 output, +20 input | input in trip range and not tripped → tripped = 1, output = 1, return 1. Otherwise: if outside the trip range and inside the reset range → tripped = 0; output = 0; return 0 (signed, inclusive) | ported |
| 11 | DelayTrigger | +0 f32 time (−1 = idle; all templates start idle), +4 s8 output, +8 trig (restart), +12 delay ms | trig ≠ 0 → time = 0. Then: time < 0 → return 0. time ≥ f32(delay) → output 1, time = −1, return 1. Else time += tick scale, output 0, return 0. A held restart re-zeroes every walk; it fires `delay` after the last walk with restart set, rounded up to a tick | ported |
| 12 | StateGenerator | +0 u16 offset of trig[] (self-relative), +2 u8 n, +4 current, +8 values[n] | the first i with trig[i] ≠ 0 → current = values[i]. Return current | ported |
| 13 | Merge | +0 u8 n, +4 trig[n] | 1 if any trig ≠ 0, else 0 | ported |
| 14 | Envelope | +0 u16 offset of control word (self-relative), +2 s8 prevcontrol, +3 u8 segment, +4 f32 remaining, +8 f32 delta, +12 f32 output, +16 u8 nseg, +18 s16 release segment, +20 f32 initial, +24 {f32 duration ms, f32 target}[nseg] | see §4.1 | ported |
| 15 | Table | +0 TABLE ptr, +4 prev input, +8 output, +12 input | see §4.2 | ported |
| 16 | DelayLine | +0 u16 offset of {value, delay ms} (self-relative), +2 u16 slots, +4 u16 in, +6 u16 out, +8 prev delay, +12 ring[slots] | see §4.3 | ported |
| 17 | Mux | +0 u8 n, +4 control, +8 inputs[n] | 1 ≤ control ≤ n → inputs[control−1]; else 0 | ported (called "stack top") |
| 18 | Demux | +0 u8 n, +2 s16 prev, +4 control, +8 value, +12 outputs[n] | outputs[prev−1] = 0 first (prev starts at 1 on all disc blocks); then if 1 ≤ control ≤ n: outputs[control−1] = value, prev = control. Return outputs[0] | ported (called "stack push") |
| 19 | Minimum (n-ary) | +0 u8 n, +4 inputs | min of inputs[0..max(n,1)] (signed) | **not ported** (unused) |
| 20 | Maximum (n-ary) | same | max (signed). Retail `sub_82B1CEF8` confirmed | **not ported** → those banks fail |
| 21 | Scale | +0 u8 n, +4 f32 scale, +8 inputs | acc = f32(in[0]); for i in 1..n: acc = f32(acc·f32(in[i])); acc = f32(scale·acc); return round(acc). in[0] is used even when n = 0 | ported |
| 22 | Add | +0 u8 n, +4 inputs | wrapping sum of inputs[0..max(n,1)] | ported |
| 23 | Subtract | +0 a, +4 b | a − b | ported |
| 24 | Multiply | +0 a, +4 b | a·b (low 32) | ported |
| 25 | Divide | +0 a, +4 b | b = 0 → 0; i32::MIN/−1 → 0; else a/b truncated | ported |
| 26 | Modulo | +0 a, +4 b | b = 0 → 0; else a − (a/b)·b with the same quotient rule (sign of a) | ported |
| 27 | Player | §1.6 | see §5 | host op |
| 28 | Oscillator | +0 u8 waveform, +4 f32 phase, +8 period ms, +12 amplitude | see §4.4 | ported |
| 29 | Ramp | +0 f32 current, +4 f32 delta, +8 prev target, +12 prev duration, +16 duration ms, +20 scale (4096 = 1×), +24 target | see §4.5 | ported |
| 30 | AddMaximum | +0 u8 n, +4 max, +8 inputs | min(wrapping sum, max), signed; in[0] used even when n = 0 | ported |
| 31 | SubtractMinimum | +0 min, +4 a, +8 b | max(a − b, min) | ported |
| 32 | MultiplyMaximum | +0 max, +4 a, +8 b | min(a·b, max) (low 32, signed) | ported |
| 33 | Minimum2 | +0 a, +4 b | a < b ? a : b | ported |
| 34 | Maximum2 | +0 a, +4 b | a > b ? a : b | ported |
| 35 | Scale2 | +0 f32 s, +4 a, +8 b | round(f32(f32(f32(b)·f32(a))·s)) | ported |
| 36 | Add2 | +0 a, +4 b | a + b | ported |
| 37 | Function | the FunctionState: +25 u8 trig, +28 values | return trig, then clear it; programs copy the values out with pairs | ported |
| 38 | ControlClass | +0 class handle (8 B), +8 post node, +12 u8 clamp flag, +13 u8 n, +16 [n × {min,max}], then inputs {construct, destruct, params[n]} | see §4.6 | **not ported** → Tazer fails |
| 39 | SetGlobalVariable | +0 global handle (8 B), +8 min, +12 max, +16 prev, +20 value | value ≠ prev → prev = value; set the global to clamp(value, min, max) (§2.7). Return 0. On disc prev = value = 7FFFFFFE initially, so nothing is published until the first real value | host op |

### 4.1 Envelope (op 14) [P, matches N]

Control word: 1 play, 2 hold, 3 release, anything else stop. Programming a segment means:
remaining = duration; delta = (target − value_from) / duration × tick scale. In f32, value_from is the
current output, or for a successor segment the target just reached.

Evaluate in this order:
- **Start:** control = 1 and prevcontrol = 0 →
  - output = initial, segment = 0, program segment 0.
- **Release jump:** else, control = 3 and prevcontrol ≠ 3 and segment < release segment (signed 16-bit
  compare) →
  - segment = low byte of the release segment; program it from the current output.
- **Run:** else, if control is 1 or 3:
  - if segment < nseg: remaining −= tick scale.
    - remaining > 0 → output += delta.
    - else → output = target[segment], segment += 1. If segment < nseg, program the next segment from
      that target; else output = 0.
  - if segment ≥ nseg: output = 0.
- **Hold:** control 2 → output unchanged.
- **Stop:** any other control → output = 0.

Finally prevcontrol = low byte of the control word, re-read after the stores. Return round(output).

**The output drops to 0 after the last segment**, so authors end on a hold or repeat.

### 4.2 Table (op 15) [V]

1. input == prev → return the cached output (no work).
2. prev = input; idx = clamp(input, min, max) − min.
3. **Resolution exactly 1.0 (nearest):** output = entry[idx], sign-extended. No bounds check: idx can run
   past count if max − min ≥ count, which does not happen on disc.
4. **Else (linear):**
   - x = f32(idx)·res;
   - i0 = round(x − 0.5). That is floor(x) for x > 0, **but −1 for x = 0**, which reads the bytes just
     before the entries (the resolution field). For s8/s16 the result is still exactly entry[0]; s32
     tables are all nearest, so this never matters on disc.
   - frac = x − i0;
   - i1 = min(i0 + 1, count − 1);
   - output = round(fused(entry[i1] − entry[i0], frac, entry[i0])) in f32.
5. Store and return the output.

nfsmw truncates i0 instead; Skate rounds. Keep Skate's rule.

### 4.3 DelayLine (op 16) [P, matches N]

1. If the input's delay ≠ prev delay:
   - prev = delay;
   - if delay < 0, write 0 **back into the input word**;
   - offset = trunc(f32(f32(delay)/tick scale) + 0.5);
   - if offset ≥ slots (signed), offset = slots − 1;
   - in = out + offset (u16).
2. Wrap: if in ≥ slots, in −= slots; if out ≥ slots, out = 0.
3. ring[in] = value; result = ring[out]; in += 1, out += 1 (u16). Return the result.
4. The write comes before the read, so offset 0 passes the value straight through.

### 4.4 Oscillator (op 28) [V table, P code]

1. period ≤ 0 → return 0, no state change.
2. inc = f32(tick scale / f32(period)).
3. While phase ≥ 1.0: phase −= 1.0. A NaN or infinite phase hangs retail; guard it in the port.
4. Sample by waveform:
   - **0 sine:** u = round(phase·1024); quadrant q = (u >> 8) & 3; i = u & 255. s = T[i], T[256−i],
     −T[i], −T[256−i] for q = 0..3. sample = f32(f32(f32(s)·amp)·(1/65536)).
   - **1 square:** phase < 0.5 → 0, else amp.
   - **2 saw:** phase·amp.
   - **3+ triangle:** phase < 0.5 → phase·amp·2; else (1 − phase)·amp·2 (f32, left to right).
   - On disc: sine 169, triangle 154, square 48, saw 1.
5. phase += inc (every path). Return round(sample).

**Quarter-sine table T (257 u16, image 0x82FD36B8) [V]:** T[i] = min(65535, floor(65536·sin(i·π/512)))
for i = 0..256. Exact on all 257 entries (checked with f64; `aems_image_consts.py` (local tool)). Generate
it; don't copy it.

### 4.5 Ramp (op 29) [P, matches N]

1. f32(target) == current → return target.
2. If target ≠ prev target or duration ≠ prev duration:
   - store both;
   - duration ≤ 0 → current = f32(target), return target;
   - delta = f32(f32(f32(f32(target) − current) / f32(duration)) · tick scale) · (1/4096).
3. current = fused(f32(scale), delta, current).
4. Overshoot: if delta < 0 and current < target, or delta ≥ 0 and current > target → current = target.
5. Return round(current).

With scale 4096 (714 of 800 on disc) the ramp spans `duration` ms. Scale 0 (79) freezes it until the
target or duration changes and a re-derive with duration ≤ 0 snaps it.

### 4.6 ControlClass (op 38) [V, retail sub_82B1C2B8]

It owns a child post: a nested class instance, e.g. c_tazer → `c_tazer_grn_play`.
- **destruct ≠ 0:** if the node exists, release it (§2.5) and clear it.
- **else construct ≠ 0:** if no node yet, clamp the params (if flag), post to the class with the params,
  and store the node (0 on failure). Construct does not post again while a node exists.
- **else** if a node exists: clamp the params (if flag) and redeliver them.
- **Return:** the node's refcount if a node exists, else 0. The refcount is 1 for the owner, plus 1 for
  each destructor or ClassData client of the child's live instances. So > 1 means "the child instance is
  alive".
- The module's numClassControllers list lets Destroy release these nodes.

---

## 5. Players (op 27) and voices

### 5.1 Op 27 per walk [P, matches N; retail sub_82B1D240]

1. c = clamp(playcontrol, 0, 2).
2. **If c ≠ prevplaycontrol[0]:**
   - c = 0: if a voice exists → release it, then clear the handle and the outputs (timeleft = timecurr
     = 0 when updateoutputs).
   - c = 1:
     - a voice exists → resume it;
     - otherwise, unless (prev[0] = 2 and prev[1] = 1) (a voice that died while paused is not
       restarted)
       - k = sampleselect clamped to [0, count−1];
       - entry k's sample index `FFFF` → no voice (outputs cleared);
       - otherwise **open** a voice (§5.2).
   - c = 2: if a voice exists → pause it.
   - Then prev[1] = old prev[0]; prev[0] = c.
3. **If c = 1 and a voice exists:**
   - Push every input whose value ≠ applied (in record order; applied = value afterwards, unclamped).
   - Query the voice. If it has ended: release the handle, clear the outputs, and the op returns 0.
   - Else, if updateoutputs: timeleft = remaining ms, timecurr = elapsed ms (and 8 extra words if +17).
4. Return 1 (playing), 2 (paused with a voice) or 0 (no voice).

**Consequences:**
- **Restarting needs a 0 → 1 edge:** a voice that ends naturally leaves prevplaycontrol at 1. To play
  again the program must drop playcontrol to 0 for at least one walk. This is how relays and re-triggers
  are built (water_fountain alternates two players).
- On open, all inputs are pushed (open applies the full set). Query and outputs happen **in the same
  walk**, so timeleft is valid immediately after the open.

### 5.2 Voice open (device `sub_824A3140`): what AEMS hands over [P]

- the sample: S10A slot = entry's s16 index, data = sfx bank + offset[index];
- the entry's level % → per-voice player gain ×0.01;
- the azimuth bytes (per channel; mono uses the first) → initial pan in 65536 = 360° units;
- the bank's stream path and (stream offset + entry offset): RAM samples only on disc;
- the input list: routing codes are read from records with id ≥ 9 (§5.4); every input is then set once.

The voice graph is out of scope here; see `rwaudio-prior-work.md` and `aems-reference.md`:
SndPlayer1 → Rechannel → Resample → HighPassIir2 → LowPassIir2 → [Send] → Gain → [Send] → Pan2D1 → Send.

### 5.3 Remaining/elapsed ("timeleft/timecurr")

Retail query `sub_824A2DD8` [V]:
- alive = the player's state byte ≠ 2;
- duration = sample frames / sample rate, from the stream header;
- elapsed = the SndPlayer status time, **only once the player has reached this voice's play request**
  (else 0);
- remaining = trunc((duration − elapsed)·1000), elapsed = trunc(elapsed·1000), in ms.

**Pitch:** duration is in source frames. The 2005 code computes time from the source frame position.
So **remaining is source time, not wall time**: at pitch p the real time left is remaining/p. This is
[V] for the duration and [N] for elapsed; Skate's status time field is not re-traced [?].
- Consequence: water_fountain (pitch 0.82–0.94) starts its relay later in wall time than a pitch-1 mock
  predicts.
- 2005 reports 0 time-left for sustained loops [N]. Skate's loop behaviour is unknown [?]: the PoC mock
  reports 1e6 ms for loops.

### 5.4 Routing codes at open (ids ≥ 9) [P]

For each input record with id ≥ 9, in order, the value v selects:
- **v ∈ {4096, 8192, 16384}:** the effect bus, 16384 = the second one. It **consumes the next two
  records:** the first must be 1 (enable), the second is the level /32767.
- **v ∈ {512, 2048}:** fixed routing buses.
- **v ∈ {256, 1024}:** nothing.
- **v ∈ 0..7:** output bus = material bus v.
- **v ∈ 10..17:** output bus = bus v−10 of the second set.
- anything else: no effect.

Effect-send level = id 11 later. Exact bus meanings belong to the mixer port.

### 5.5 Setter clamps and units (summary)

See §1.6. id 2 is the master level, and dry (8) and send (5) are each multiplied by it.

---

## 6. Per-bank behaviours to reproduce (measured on the PoC)

All with the PoC mock device:
- a voice is alive while now < open time + sample duration;
- remaining/elapsed come from wall time at pitch 1;
- c_emitter payload `w0..w8 = 32767,32767,0,0,4096,25000,0,0,<selector>` unless stated.

| Bank / class | Measured (PoC) | Notes |
|---|---|---|
| water_fountain (sel 81) | **17 starts in 30 s** (re-run 2026-10-02): 0.059, 1.787, 4.091, 5.531, 7.163 … 28.923 s. Each new voice opens 192 ms (6 walks) before the previous ends | `ems-emitters-re.md` says "about 24 in 30 s": **not reproduced**, treat 17 as the PoC truth. Retail spacing will be longer (remaining is source time, pitch < 1) |
| water_lapping (sel 82) | 23 starts in 30 s | same relay |
| Siren_city_1 (sel 225, w0 = 20000) | 2 layers: 0.027 s and 0.219 s = 6 walks apart | retail Siren_city_4: +16 / +216 ms (recomp trace) ✓ |
| c_main_ambience_crossfade (w9 = group) | 4 voices open in the same walk (one group of 4) | 7 groups × 4 voices in DT/Ind/Uni |
| c_emitter_utility | 2.5 s timers re-drawing random_pitch/vol globals | function round trip broken in PR #4 (§11) |

---

## 7. Implementation outline for the native port (behaviour, not code)

- **Load:** parse the csi files into symbol tables. Parse each bank, apply rebases as offsets, resolve
  interface references into handles, and register modules on class records. Reject opcodes ≥ 40 and
  programs whose block walk ≠ datasize.
- **Instance memory:** a byte buffer per instance (template copy). Ops read and write big-endian words in
  it at block offsets, exactly as on disc, so programs need no translation.
  - Pointers inside templates (TABLE, sample group, bank) stay bank-relative and resolve through the
    bank.
  - Handles become {table, index, generation} triples in a side table keyed by their offset.
- **Clients:** destructor, ClassData, GlobalVariable and Function subscriptions hold (instance id, state
  offset). Calls are synchronous, as in retail.
- **Tick:** counter-based (every 6th block), tick scale 31.999998 f32. The walk runs over a Vec of
  instance ids in newest-first order; removals take effect immediately but must not skip the next one.
- **Voices:** a trait with open/release/pause/resume/set/query.
  - The game backend maps it onto our mixer.
  - The test backend is the PoC-equivalent mock (§8.2).
  - Query must report **source-time** remaining for retail parity.
- **RNG:** a struct {w0..w5}, zero at engine start, one per engine; deterministic.
- **Posting API for game code:** post(class, payload) → node id; redeliver(node, payload);
  release(node); call_function; set_global.
  - Posts and redeliveries from the game thread are queued and applied at block boundaries, before that
    block's tick.

---

## 8. Test plan

### 8.1 Unit tests per op (no banks)

- One test per opcode from this spec: edge cases (empty counts, out-of-range controls, the wrap paths,
  Envelope start/release/end, Table x = 0, DelayLine delay change and offset 0, Ramp overshoot and
  scale 0, Oscillator quadrants 0–3 and u = 1024).
- RNG: the first draws from zero are 0, 1, 7; state after 3 draws = [7, 6, 5, 4, 3, 3]. Test a full
  ripple (w5 = FFFFFFFF, others 0).
- Shuffle: span arithmetic and "no repeat across the wrap" over 10,000 draws.
- Period: δ = f32(256/48000), D = 30 → count 6, tick scale 31.999998; the first walk on the 6th call.

### 8.2 Golden vectors from the PoC evaluator

Use the PoC's `emitter_probe` (PR #4 engine in a local worktree; local only, never
committed; build incrementally in its own target dir).

**Fix the PoC first (local edits only):**
1. **op 5:** pass the block's +0 handle, not the inputs word (§11). Without this no Function is ever
   delivered.
2. Implement **ops 19, 20 and 38** per §4.
3. Add a trace hook to the interpreter. Per walk, log: walk number, block index, time; then per instance
   (in walk order): module name, and for each op: opcode, block offset, result. Add a hex dump of the
   instance buffer every N walks.
4. Log the 6 RNG words before every draw.
5. Keep the mock device:
   - alive = now < start + duration;
   - remaining = trunc((end − now)·1000), elapsed = trunc((now − start)·1000);
   - loops alive forever with remaining 1,000,000;
   - now = block index × f64(f32 δ).
   - Also log OPEN (sample index, level %, azimuth bytes, stream offset, record count), SET (id, value),
     QUERY results, RELEASE, PAUSE and RESUME.

**Capture set (one post each; 30 s unless noted; seed RNG = 0; δ f32):**

| Bank(s) | Post | Exercises |
|---|---|---|
| water_fountain, water_lapping, water_lapping_pond | c_emitter sel 81/82/83 | relay, shuffle (8), table on timeleft (15), oscillators (28), Scale2 (35), Player restart edges |
| trees_rustle, pub_amb | c_emitter sel 76/33 | loop player, AddMaximum gusts, single start |
| Siren_city_1 + emitter_utility | utility at boot (`PROBE_UTILITY=1`), then c_emitter sel 225; post twice 25 s apart | Function call (5/37), shuffle → SetGlobalVariable (39) → GlobalVariable (2) → sampleselect, the "previous draw" rule (§2.7) |
| DogBarks_medium, misc_bangs_distant, Bird_calls_song | with utility | more function/global pairs, weighted/random picks |
| Main_Ambience_Crossfade_DT | c_main_ambience_crossfade, w9 = 1..7, plus a w9 change mid-run | Mux/Demux (17/18), quad pans |
| GRINDS | Class_grind post + 6-walk redeliveries (PR #4 grind_instance payloads) | redelivery, ramps, StateGenerator |
| fstep_skateshoe1_sm | playercharacter_footstep with the msgs1 payloads | many players in one walk |
| Foley_Cloth | cloth_trick | op 39 with player banks, op 5 |
| Seams_Bank, Bodyslide, Security_Alarm_1 | their classes | Envelope (14) |
| PatchBank_RocksBounce, Traffic_Skid | Class_rolling / traffic | RandomWeighted (9) |
| car_alarms, arena_cheers | their classes | Maximum n-ary (20) |
| Tazer | c_tazer, release after 3 s | ControlClass (38) create/update/release, child lifetime |
| any c_emitter bank | post, release at 5 s | Destroy on release, voices released |

Store the goldens locally (one text file per bank), never committed. The native harness replays
the same posts, redeliveries and releases at the same block indices, uses an identical mock device, and
must match **every op result and every device call bit for bit**. Diff the first divergence and stop.

### 8.3 Retail checks (later, passive only; no repeat sessions)

Recomp trace hooks:
- the RNG words at the first AEMS draw (seed question);
- the δ passed to `sub_82B1E290`;
- `sub_824A2DD8` outputs for a pitched voice (source vs wall time, loops);
- op-5 calls (`sub_828E29C0` handle + params);
- op-39 publishes.

Compare to `Siren_city_4` layer times (+16/+216 ms) and the location-set firings already recorded.

---

## 9. Constants (facts)

| Address | Value | Use |
|---|---|---|
| 0x82FD3600 | 40 × u32 | opcode table (slot → function: see PR #4 eval/mod.rs table) |
| 0x82FD35F4 | f32 41.6 boot → 30.0 after `sub_826D4C30` | period denominator D |
| 0x8231A844 | 1.0 | period numerator, phase wrap, "nearest" test |
| 0x82256FE8 | 1000.0 | tick-scale unit (ms) |
| 0x830775D8 / DC / E0 / E4 | tick scale / cached δ / period / countdown | evaluator timing |
| 0x830775F0 | 6 × u32, 0 at boot | AEMS RNG |
| 0x83036F4C | list head | instances (timer clients) |
| 0x8216DEE0 | −1.0 | DelayTrigger idle |
| 0x822F890C | 1/4096 | Ramp unit |
| 0x822F8EA4 | 1024.0 | sine phase scale |
| 0x82098D0C | 1/65536 | sine normalisation |
| 0x82060C50 | 2.0 | triangle gain |
| 0x8209975C | 0.5 | rounding |
| 0x82FD36B8 | 257 × u16 | quarter sine (formula §4.4) |
| 0x822F8890 | f64 1000.0 | query ms unit |
| 0x8302D4A4 / 0x8302EE28 | 72 game classes / handle slots | game-side object table |

Functions: tick+interpreter `sub_82B1E290`; bank install `sub_82B1DF50`; post `sub_828E2B48`; redeliver
`sub_828E2D18`; release `sub_828E2C78`; call function `sub_828E29C0`; set global `sub_828E2F38`; create
instance `sub_82B1DAD0`/`sub_82B1D880`; destroy `sub_82B1C150`/`sub_82B1BF98`; RNG draw `sub_82B1F360`;
player `sub_82B1D240`; voice open `sub_824A3140`; voice query `sub_824A2DD8`; csi install `sub_828E2818`.

---

## 10. Open questions

1. RNG initial state in a live retail session (expected all zeros).
2. Remaining-time semantics for pitched and looping voices (status time = source position?).
3. Sample-group azimuth bytes 4–8: per-channel angles, or `{…, slot+1, FF}` metadata?
4. Player +16 (sampletype `FF`) and +17 (extra outputs): any Skate reader?
5. Who in game code sets globals or calls functions directly (e.g. the "G" writer)?
6. Siren second layer: does it read the freshly published `*_snd` value (§2.7)? Golden plus retail.
7. Header byte 9 = 3, and the export kind's low 24 bits.

## 11. Corrections to earlier notes and to PR #4

- **PR #4 op 5 bug [V]:** retail `sub_82B1C210` passes the **block's own +0 handle** to the
  function-call helper, with params at inputs+4. PR #4 passes the inputs word (the trigger flag) as if it
  were a handle.
  - So every CallFunction is either skipped (trigger 0) or treated as an "unmapped symbol 1".
  - The "literal 1 placeholder in Rolling_Rattles" in PR #4 docs is this bug.
  - Result: no `*_msg` is ever delivered in PR #4 or the PoC, so `*_snd` sample counters never advance.
    PoC data for function-driven banks (sirens, dogs, birds, rolling broadcasts) is suspect wherever
    sample choice matters.
- **Ops 17/18** are Mux/Demux, not a stack. **19/20** are n-ary min/max (20 used by 4 banks, not by the
  Rolling banks as `aems-reference.md` said). **38** is ControlClass, used once (Tazer).
- **Header +0x08** is platform byte 5 + byte 3 + u16 module count, not a "variant word"
  (0x05030001/2/3 = 1/2/3 modules).
- **"Pure ops then host ops":** wrong. Ops run strictly in program order.
- **Csi tables:** 0 = Functions (`*_msg`), 1 = Classes, 2 = GlobalVariables (with default value).
- **Tick scale** is 31.999998, not 32.0. Remaining time is in ms of **source** time.
- **water_fountain** "24 starts / 30 s" did not reproduce; the PoC gives 17.

## Helper scripts

Local scripts (the survey is published as `tools/audio-file-inspect/aems_survey.py`, `fn.sh` as
`tools/recomp-code-search/fn.sh`):
- `aems_survey.py`: header/module/program/template/curve census and invariants; `--dump X.abk`.
- `aems_image_consts.py`: image constants, RNG words and sine-table formula check.
- `fn.sh`: dump a recompiled function.
