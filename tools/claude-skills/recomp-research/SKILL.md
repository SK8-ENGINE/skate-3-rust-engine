---
name: recomp-research
description: Reverse-engineer any part of retail Skate 3 (NPCs, traffic, AI and aggression, triggers and teleport volumes, physics, audio, scripting...) by finding the code in the TU3 recompilation, hooking it with guarded trace hooks, driving the game with scripted runs and analysing the trace, then porting the behaviour to the Rust engine. Use whenever the engine needs retail behaviour that isn't documented yet.
---

# Recomp research: find -> hook -> run -> analyse -> port

Related skills: `recomp-audio-trace` (build, layout, title update), `recomp-scripted-runs` (pad scripts, teleports,
batches, screenshots), `ghidra` (pseudocode of retail functions), `aems-port` (an example of a full port),
`prior-work-check` (always first).

Credits: skate3recomp (mchughalex), its rexglue SDK, and Xenia; Ghidra (NSA) and Xenon360 (EveryoneEverybody) when
you read code through skill `ghidra`. Credit them in every doc and PR that uses findings.

## Rules
- **Your own copy, your own build:** you need a legally owned copy of Skate 3 and must build the recomp yourself
  (skill `recomp-audio-trace`). Nothing from the game is shared or committed.
- **Reference only:** skate3recomp has no licence. Never copy generated or recomp code (or Ghidra pseudocode) into the
  engine; describe behaviour in your own words (addresses, constants and offsets are facts and fine). Disassembly
  stays local.
- **Retail code is the source of truth.** The recomp is not the console: its frame rate, threading and timing differ.
  Trust its logic and values; treat its timings with care. Label every ported value with where it came from
  ([code] address, [data] record, [trace] session).
- **One game at a time:** at most one recomp or one short skate3rust test. Never build the recomp while it runs (the
  exe is locked).
- **Nothing windowed while someone plays** on the machine: no skate3rust or recomp launches, and background helpers
  run headless or are stopped first.
- **Clean up:** stop processes and remove large temporary output; `SKATE_REPORT_CHILD=1` for skate3rust runs.
- **Scripts live in the repo** (a gitignored `.local/research/<topic>/` for data, a skill's `tools/` for reusable
  scripts), never only in a temporary scratch folder.
- **Guard every hook read** with `Readable()` (an SEH page probe). An unguarded read through an inferred pointer
  crashes the recomp.
- **Scripts are open-loop:** they can teleport and act in place, but cannot steer to a target (a "wander" script got
  stuck in a corner). Anything that involves reaching NPCs or places needs a coordinate teleport (below) or a passive
  session recorded while a person plays.
- **Confirm the mechanic exists in Skate 3** before chasing it: some systems (for example security guards) are
  Skate 2 leftovers that Skate 3 does not use. Verify claims either way with a trace.

## 1. Find the code
- **Ghidra pseudocode first:** skill `ghidra` turns a batch of TU3 function addresses into pseudocode in about 10 s
  (`decompile.ps1 -Out <file> <addr> ...`). Much faster to read than PowerPC; confirm offsets, constants and loop exits
  in the recomp's generated code before porting.
- **Strings, RTTI and descriptors** in a dump of the TU3 memory image (big-endian, base 0x82000000) made from your
  own copy: class names (`CSTATE_*`, `SFXObj_*`, ...), debug strings, vtables. Then find the code that uses them in
  `<recomp checkout>/generated/skate3_recomp.*.cpp` (`DEFINE_REX_FUNC(sub_X)`).
- **Addresses in code:** `lis rN,<hi>` + `addi`/`lwz <lo>`. The high half is sign-extended: `lis -31992` = 0x8308,
  `lis -31987` = 0x830D. Globals live at 0x8308xxxx / 0x830Cxxxx; easy to confuse with 0x8208/0x820C. Large offsets
  split as `addis rX,rY,3` + `-3920`, i.e. 0x2F0B0. The low half is signed too: check every hi<<16 + lo sum twice.
- **Code search tools:**
  - `tools/fn.sh ADDR < /dev/null` (dump a function's asm comments; needs `RECOMP_GENERATED`). Always pass a non-empty
    address and redirect stdin: with an empty address awk reads stdin and hangs.
  - `tools/fnstrings.py ADDR` (data addresses a function materialises and the string at each; needs `PPC_IMAGE`).
  - AI-graph ops: an op's vtable comes from its constructor's `stw rX,0(r31)`, with Begin / Update / End at
    vtable +48 / +52 / +56 (a condition's test is +48); when a factory has no vtable store, follow its `bl` to the real
    constructor.
  - `grepfn.sh`, `callctx.sh`, `ppcxref.py` from `tools/recomp-code-search/` in the repo (`ppcxref.py`: `IMG` is
    the image, read vtables and floats with `struct`; `cstr`, `callers`, `xrefs`).
- PowerPC `bdzf` = decrement CTR, branch if CTR == 0 and the condition is false (switch idiom); reading it as
  "CTR != 0" swaps the cases.
- **Data:** the attribute database (`assets/private/stock/skater-collections.json` in your converted install; class and
  record names are listed in the disc's `db.big` summary report `skatercollections_summaryreport.txt`; keys are
  `name_id` = lookup8 of the name), world-painter region layers (`tools/asset_pipeline/audio_formats
  .region_layers`: districts, census, challenge zones...), streams via `skate3_streams.read_sfil`.
  Names inside a district's sim streams: search the extracted `cSim_*.xsf` files for printable strings.
- **APT menu movies:** in action bytecode, ops 0xA2 / 0xA3 / 0xAE / 0xAF / 0xB0..0xB3 index the function's own 0x88
  constant pool, not the movie's CONST table; a disassembler that resolves them through CONST prints the wrong names.
- **Delegate wide searches** to background agents with a precise brief (sources, addresses, the "own words" rule,
  output paths). Several agents can map different subsystems in parallel.

## 2. Hook it
- **Where hooks live:** the recomp's `src/research/`:
  - `trace_common.h`: guarded reads `Readable`/`TryU32`/`TryU64`/`TryF32`/`Vec3Text`, `CallerChain`, `NameAt`,
    `HexAt`, the category gate `On("<cat>")`, and `FIRST_PASS_HOOK`;
  - `hooks_<domain>.cpp` per domain (audio, physics, watch, world, npc, traffic, vehicles, marker, queue, cas, grind,
    render, ...), added to the common sources list in `CMakeLists.txt` (reconfigure after adding one).
  - A published set exists as a `research-hooks` branch of a skate3recomp fork (linked from the repo's
    `tools/recomp-code-search/README.md`). If you publish your own: no game code or data, no private paths, SDK
    changes as a patch file, a README kept current.
- **Rebuild:** from PowerShell in the recomp folder with the build helpers of skill `recomp-audio-trace`
  (`recomp_build.bat skate3`); `cmd //c` from Git Bash breaks on a space in the path. Only rebuild when no
  `skate3.exe` runs.
- **Hook shape:** `extern "C" REX_FUNC(sub_X) { if (On("npc")) {...} __imp__sub_X(ctx, base); ... }`. Include
  `"trace_common.h"` and `using namespace skate3_research;`. Arguments: r3-r10 / f1-f4 on entry; read struct fields
  AFTER calling through for outputs. `CallerChain` names the callers.
- **Unknown argument meanings: first-pass hooks.** Log raw r3-r8/f1-f2 on entry, r3/f1 after the call and the
  callers, rate-limited per function: one line `FIRST_PASS_HOOK(addr, "<category>", "<KIND>", "tag\t")`. Summarise
  with `tools/first_pass.py` (distinct values per field: constant = object/flag, varying = data). Then name the
  fields. Faster than decoding every argument statically. Write C++ with an editor tool, not shell/Python heredocs
  (escapes like `\t` turned into real tabs or literal characters several times).
- **Hooking a shared helper for one caller:** check `ctx.lr` against the return address after the `bl` (find
  `ctx.lr = 0x...;` before the call in the generated code), for example a set-diff helper (`sub_82557600`) only when
  called from the trigger update (lr 0x82DD7290). Non-volatile registers (r14-r31) still hold the caller's values
  after the call. Before filtering an argument by address range, check every caller's `addi rN,r1,...`: callers may
  pass STACK copies instead of the global table (a bail hook logged nothing until it accepted the per-skater loop's
  call by its lr).
- **Read flags at the width the game reads them** (`lbz` = byte). Reading a word where the game reads a byte once
  let NPC skaters through a local-player test. The reliable local-rider test for player audio objects is the byte
  `[[obj+28]+72]` ("local72"); NPC skaters' components have 0 there. Have such hooks log each object's decision when
  it changes, so old traces can be re-filtered by object if a gate turns out wrong.
- **Per-player hooks across classes:** a test proven on one class's layout (for example `[[owner+16]+72]` on the
  SkateBoard object) need not hold on another class; gate on local72 and log a mask of which tests held, so
  analysis can check.
- **One writer per process:** the exe and the runtime DLL both contain the trace header; let the first module own
  the file (named-mutex election), open it in binary append, run the only writer thread and publish a plain C
  `append` function pointer the other module calls. No STL/heap across modules. Never paper over malformed lines in
  parsers; fix the writer.
- **Light mode:** format into a local buffer and queue; a background thread writes and flushes every 100 ms. `On()`
  is a table parsed once. Rate-check BEFORE any guarded read or `CallerChain`; never hook a function called
  thousands of times a second unless the work after the rate check is tiny; prefer one sampled read per frame over
  hooking hot getters.
- **Audio command queue:** hooks on the audio render thread run inside the queue consumer's lock; keep them
  lock-free and light. Heavy tracing there can overflow the game's audio command queue; growing the queue while
  tracing and logging its fill level helps.
- **Categories:** `SKATE3_TRACE=audio,audiox,dsp,dspmod,physics,world,npc,traffic,aiskater,skitch,cas,grind,render,
  watch` (unset = all). Choose only what a run needs: DSP/MOD and physics lines alone produce millions of lines per
  long batch. High-volume per-frame detail goes in its own category (`audiox`) so normal sessions stay light.
- **Wrapping a class's update to attribute posts:** wrap the vtable process / update functions of the objects you
  care about and keep a thread-local "object being run"; any POST or Splice start inside them is then attributed to
  that object (used for per-player sound instances: local rider and nearby NPC skater).
- **Render hooks** (category `render`): one frame of effect applies and the scene views' draw lists, armed a delay
  after the first frame so the menus are past (`SKATE3_DRAWORDER_DELAY_MS`), plus before/after probes on the scene
  view methods to find the writer of a render global.
- **Extending a kind:** add a NEW kind instead of changing an existing kind's layout, so readers stay stable. Every
  kind gets a fixed field count in `tools/trace.py` `FIELD_COUNTS`; `py -3.13 tools/trace.py <trace>` prints
  malformed counts per kind (must be 0). Document every kind in the hook file header and in skill
  `recomp-scripted-runs`.
- **Proof of installation:** have each new hook write one `HOOKARMED` line on its first call, and check it fires in
  a short background run of your own before asking anyone to record a session.
- **Watch list (no rebuild):** `SKATE3_WATCH=<file>` with `<label> <type> <chain> [every=<ms>]`. Types: u8 u16 u32
  s32 u64 f32 vec3 hex<N>. Chain: hex start, `@` = load pointer, `+N`/`-N`. Evaluated on the audio tick
  (`sub_82485190`), so it also runs in menus and replay (game-logic hooks do not) and suits state checks across
  menus. Logs `WATCH <label> <value>` on change. A global can be watched from an existing per-frame hook at a few
  lines a second (on change + 1 Hz heartbeat); `CallerChain` cannot name the writer of a global (that needs a
  write-watch).
- **Line format:** `rex::audio_trace::line("KIND", fmt, ...)` (shared file and time base, `MARK` / `CLOCK` /
  `CAPTURE` lines). Kind names are short upper case with a domain prefix (`WP*`, `AMB*`, `NPC*`, `TRIG*`, `VEH*`,
  `CAS*`). Log on change or rate-limited (for example a position every 500 ms).

## 3. Drive the game
- Skill `recomp-scripted-runs`: pad scripts, the Challenge Map teleport route (4-5 stops per boot), coordinate
  teleports, chained runs as background jobs with hard time limits, screenshots read by `CLOCK`, closing the recomp
  and its overlay menu. Always pass absolute paths to the runner.
- Several hooks fire on worker threads; check malformed counts (must be 0 with the single writer).
- Discount recomp artefacts: state that lingers after teleports, character pieces left on the ground on long runs,
  audio-thread stalls (voices queued during a stall start together).

### Getting to a spot: teleport there
Use coordinate teleports to get close to an issue spot or anything that needs travelling to, instead of menu
teleports, scripted skating or asking someone to drive.
- Retail (recomp): `SKATE3_TP_OVERRIDE="x y z yaw[; ...]"` (skill `recomp-scripted-runs`, "Teleport to any
  coordinate"): each Challenge Map menu teleport lands at the next pose. Add about 4 m to an estimated ground y and
  read the settled height from the trace; points outside the playable area put retail into the out-of-bounds
  re-place loop (skater nearly invisible, frozen).
- Engine: `sdk.player.teleport(...)` / `sdk.camera.set(pos, look_at)` from a tiny local test mod with `--verify`
  captures (skill `build-and-run`), or `--teleport <id>`.
- Use the same pose in both for matched retail vs engine shots; save the poses next to the evidence.

## 4. Analyse
- Parse by kind; window by `MARK at <stop>`; align with screenshots.
- **Shared reader:** `tools/trace.py`: `Trace(path)` gives lines by kind, `stops()`, `by_stop(kind)`,
  `shot_for(ms)`, `capture(a, b)` (the float32 audio capture); run it directly for a kind and stop summary plus
  malformed counts.
- Examples of analysis scripts: skill `audio-tuning` (`check_sets_vs_trace.py`, `long_run_stats.py`,
  `check_bed_level.py`) and skill `aems-port` (`grec_level.py`, `tools/e2e/retail_windows.py`). Useful shapes for
  your own: per-bail and per-region impact summaries (flag host hitches over 40 ms and windows an NPC opened),
  collision posts by poster and owner, posts per player instance (local vs NPC), per-ped timelines (mood hits, named
  timers, chase windows, takedowns) with the nearest screenshot.
- **Filter by local72 before comparing rates:** steady riding gave 0 local body-collision posts; NPC skaters nearby
  post about 0.7 per second; the rider's own come from big-drop landings and bumping objects (cars).
- **Compare against the engine's own data or behaviour** and report agreement numbers (for example "1,859 of 1,859
  samples"). Never just eyeball a result.

## 5. Record and port
- Notes per topic (addresses, data layouts, behaviour, open questions, hook plan) in your notes folder; a scrubbed
  spec (no private paths, no game data) in the repo docs when code will cite it.
- Port in your own code, data-driven (retail values as defaults from setup data, overridable by mods), then skill
  `regression-check`, a docs entry with credits, and keep the PR description current.

## Worked findings (examples of what this method produced)
- **Solver iterations:** hooks on the iteration-count writer (`82763E00`, slot told apart by lr `8285A1E8` /
  `8285A1F8`) and the frame update (`82859E70`) showed retail keeps 50 constraint iterations in all gameplay (riding,
  bails, walking, respawns, after menus and replay); menus and Instant Replay stop the frame update. The mode is
  chosen when the owners are built (`8275DCC8`, constructor `827632A8`, third argument) from the byte `0x83082929`:
  0 gives slot 0 mode 3 / slot 1 mode 0, non-zero gives modes 1 / 2 (both 25). It read 0 in every sample; slot 1 was
  never written or solved.
- **Bail body impacts:** the ragdoll's per-region impacts are visible in the skeleton-collision pass
  (`sub_82BD60C8`); retail scales them by a COM-speed curve and takes |dv.n|. Skeleton velocities there are finite
  differences times 60. What sounded "missing" twice was an unported step, not physics.
- **Peds:** angry marker -> chase -> takedown -> gloat is real ped aggression (mood evaluator results and named ped
  timers); NPC skaters skitch cars unprompted in busy areas.
- **World one-shots from code plus existing traces:** some sounds show up as Splice starts rather than POSTs (for
  example a ped body fall keyed by an animation event), so search both kinds before calling a sound missing.
- **Online gating:** ambient peds, traffic and NPC skaters do not spawn in online games; culling still runs.
- **Engine vs retail collision at one spot:** when the engine's collision behaves differently from retail somewhere,
  hook retail's per-triangle wheel contact (`WHLC`, area-filtered by `SKATE3_WHLC`; skill `recomp-scripted-runs`),
  teleport there with `SKATE3_TP_OVERRIDE` and a short pad script, and compare its runtime triangle flags and edge
  cosines with the engine's from the asset probe `park_layout_fused_probe` (same corners, same order). The
  difference named the missing step (the collision fuser's ground plane pass); the values that step uses were then
  read with a second small hook instead of guessed.
- **Speech echo:** ped and main-cast speech has an echo path: a pre-gain send, then high-pass, a delay from the camera
  distance, low-pass, into the environment bus. It was settled from code plus existing traces, without new sessions.

## Replay camera hooks (2026-10-25, category `replay`)
`RCAMN` (calls per second per caller), `RCAMF` (the frame `82C84CF8` returns), `RCAMRAW` (the raw camera matrix
`82672118` copies), `RCAMP` (the per-frame resolve `82C8C470`). Hook each function once across the research hook
files: grep for `sub_X` before adding one, and call a helper from the existing hook when it is taken.
