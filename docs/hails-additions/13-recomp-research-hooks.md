# 13. Research hooks for the Skate 3 recompilation

## What it is

Much of the audio work in this fork, and the NPC, traffic and world research, was measured with trace hooks in a
fork of [skate3recomp](https://github.com/mchughalex/skate3recomp), the static recompilation of the Xbox 360 game.
The hooks wrap the recompiled game's functions, call straight through, and only read memory. They log what the
game does to a tab-separated trace: sounds posted and played with their levels and DSP values, the rolling bed,
board contacts, seams, skids, emitters, pedestrians, traffic, trigger volumes and teleports. They can also capture
the mixed audio output.

They are published on the `research-hooks` branch of
[Hailey-Ross/skate3recomp](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks). The hooks are in
`src/research/`, and the SDK side ships as `src/research/sdk/rexglue-sdk-research.patch`. No game code, executable
image or game data is included.

## Warning: reference and information gathering only

The recompilation is **not** a perfect copy of the console game, and its traces are not ground truth.
- **Frame rate:** it runs uncapped (several hundred fps on a PC, against about 30 on the 360). Anything the game
  does once per rendered frame happens at a different rate. Examples: the seams process, the MixMap tick and
  one-frame trigger pulses.
- **Timing and threading:** these differ from the console. The audio thread can stall for up to about a second,
  and the game hitches when streaming at speed.
- **What to trust:**
  - logged values, data and program logic are strong evidence;
  - timings, per-frame rates and gaps are specific to the recompilation until checked against the code or the
    console.
- **Our rule:** retail parity comes from the mechanism (the code and data). Traces only validate it. Behaviour
  that runs per rendered frame targets the 360's ~30 fps cadence, made independent of the frame rate.

## Using it

1. Build skate3recomp from the `research-hooks` branch as its README describes. You need your own legally owned
   copy of the game; the code generation step creates the recompiled sources locally.
2. From `third_party/rexglue-sdk`, run `git apply ../../src/research/sdk/rexglue-sdk-research.patch`, then build.
3. Set `SKATE3_AUDIO_TRACE_FILE` (the output file) and, optionally:
   - `SKATE3_TRACE`: the categories, e.g. `audio,dsp,npc,world,traffic,aiskater`; `audiox` for heavy per-frame
     detail;
   - `SKATE3_AUDIO_CAPTURE`: mixed output, float32 stereo at 48 kHz;
   - the input script and record variables;
   - `SKATE3_BACKGROUND`.

   The branch's `src/research/README.md` lists every variable and line kind, and
   [`src/research/USAGE.md`](https://github.com/Hailey-Ross/skate3recomp/blob/research-hooks/src/research/USAGE.md)
   is a step-by-step guide: a first trace, choosing categories, reading and checking a trace, watch lists, input
   scripts and the audio capture.
4. Check every trace for malformed lines before using it; there should be none.
5. Traces made before 2026-10-03 ~10:30: the per-player kinds (GREC, GRECX, FIRSTHIT, SKID, TREAT, SEAMPAT, SEAMHIT)
   also logged a nearby NPC skater's objects, interleaved per frame. Filter them by object (the first one logged is
   the local rider). Newer builds gate on the game's local byte `[[object+28]+72]` and log `LOCALTEST` lines; see
   doc 11, "The per-player recomp hooks logged an NPC skater too".

The analysis scripts we used are published in `tools/recomp-trace/` (trace readers, per-bank level and voice
tools, rolling-bed and send analysis) and `tools/recomp-code-search/` (searching the recompiled sources and the
memory image); see [14](14-published-tools.md). They are reference only: anyone who wants to use them needs to
build the recomp and set up the paths themselves.

## Credits

[skate3recomp](https://github.com/mchughalex/skate3recomp) by @mchughalex,
the [rexglue SDK](https://github.com/rexglue/rexglue-sdk), and [Xenia](https://github.com/xenia-project/xenia)'s
Xbox 360 research.

## Playing it from the couch, and closing it

- Our couch launcher (local, `.local\steam\`) starts the traced recomp from Steam / Steam Link with one shortcut per
  trace mode, checks each trace for malformed lines afterwards, and keeps one game running at a time.
- The recomp's settings overlay opens with **Escape** or **F1** and has **"Quit to the desktop"**; it is
  controller-navigable once open. On the `research-hooks` branch, holding **LB + RB + Back** for ~1 s opens it from the
  controller (cvars `skate3_menu_pad_chord` / `skate3_menu_pad_hold_ms`); a quick tap doesn't. Back alone opens Skate 3's
  Instant Replay, which shows underneath while holding. Start isn't used because it opens Skate 3's own pause menu.
- Closing with Steam's "Exit game" is safe for the data (the trace is written continuously).
- A heavy trace (`audio,audiox,dsp` plus the audio capture) can coincide with stalls; the trace writer stops before a
  stall, so a stall can be missing from the trace. Drop `dsp` if stalls recur.
