# Recomp trace analysis

**For use with the Skate 3 recomp's research hooks:**
[Hailey-Ross/skate3recomp, branch `research-hooks`](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)
(described in [`docs/hails-additions/13-recomp-research-hooks.md`](../../docs/hails-additions/13-recomp-research-hooks.md)).

> **Reference only.** These scripts read traces written by that branch's hooks. You must build the
> recomp yourself (from your own legally owned copy of the game), apply its SDK patch, and set up the
> trace paths and the paths below for your machine. The recompilation is not a perfect copy of the
> console game: it runs uncapped, and its timing and threading differ. Treat logged values, data and
> program logic as evidence; treat timings and per-frame rates as specific to the recomp.

The hooks write a tab-separated `trace.tsv` (`KIND <tab> <ms> <tab> fields…`, all on one time base),
optionally a float32 stereo capture of the mixed output, and screenshots. A session folder holds
`trace.tsv` (and `trace.f32` / `audio.f32`, `shots/`).

| Script | What it does |
|---|---|
| `trace.py` | Shared reader (`from trace import Trace`): lines per kind, MARK stops, screenshot nearest a time, capture windows. Run alone: line counts per kind, malformed fixed-layout lines (should be zero), stops. |
| `first_pass.py` | Summarises first-pass raw-register hook lines per kind: call counts and the distinct values of each register, so constant fields (pointers, flags) stand apart from data. |
| `resolve_trace.py` | Finds each played sample's payload in your own disc's audio archives and names the bank member and stream. Used by `retail_voices.py`. |
| `retail_voices.py` | Joins each voice's PLAY with its MOD (pitch, filters), GAIN and SEND lines: per bank, voices, pitch / rate quantiles, filter cutoffs and level. `--csv` per voice; `--modules` lists the buses. Caches resolutions in the session folder. |
| `retail_relay.py` | How one emitter bank's voices follow each other: starts, slots, pitch, gaps, overlaps, repeats and the most voices at once. Needs `play_resolve.json` from `retail_voices.py`. |
| `grec_level.py` | Grain-bed (rolling) levels from GREC lines per speed band: straight rolling and carving. |
| `grec_clean.py` | Grain-bed gain on clean straight rolling only (settled, no brake / push / balance / trick), per speed band and truck. |
| `grec_material.py` | Wheel material against wheel count from GREC lines: how often the material changes while rolling, and how often it reads the "no contact" value. |
| `send_vectors.py` | Per-channel send gains from SEND lines: power sums, LFE share, most common pan shapes. |
| `grain_trace_stats.py` | Grain voice starts (PLAY lines matching a `.grain` member of your disc): starts per member, per second and inter-start intervals. |
| `retail_windows.py` | Straight four-wheel rolling windows of the session's capture per 5 km/h band (level, octave bands), next to the engine's e2e renders folded the same way (`tools/audio-e2e`). |
| `veh_trace.py` | Traffic summary from the traffic hook lines: per vehicle speed, target speed, manoeuvre and lane changes, stops, plus the horn / skid / engine lines. |
| `bail_impacts.py` | Per bail (BAILSTEP / BAILREG lines): the ragdoll's update interval, then per body region the impacts the game wrote, the per-update velocity change along the contact normal (percentiles), whether the largest ones are contact stops or drives, and the body / cloth / concrete sound posts in the bail. Flags hitches over 40 ms. |
| `collision_posts.py` | Collision-sound posts (COLLPOST) split into bail windows and the rest, per posting function and owner: rates, material pairs, tiers and the local-rider flag (`local72`), so the player's own sounds can be told apart from NPC skaters' and props'. |

## Inputs

- A session folder or `trace.tsv` from the research-hooks build.
- Your extracted disc for `resolve_trace.py`, `retail_voices.py`, `grain_trace_stats.py`: `--disc DIR`
  (the folder holding `data/`), default `$SKATE3_DISC` or `.local/skate3-disc`.
- An installation's decoded banks for `retail_relay.py` (`--assets`, default `assets`).
- Hook-specific values that depend on your build are inputs, not built in: e.g.
  `retail_windows.py --contact-callers` (the caller prefixes of the board-contact SPLC lines in your trace).

## Usage

```
py -3.13 tools/recomp-trace/trace.py sessions/my_session/trace.tsv
py -3.13 tools/recomp-trace/retail_voices.py sessions/my_session --bank SomeBank --csv voices.csv
py -3.13 tools/recomp-trace/retail_relay.py sessions/my_session SomeBank --from 60000 --to 120000
py -3.13 tools/recomp-trace/grec_level.py sessions/my_session/trace.tsv
py -3.13 tools/recomp-trace/retail_windows.py sessions/my_session .local/audio-e2e --contact-callers 82AAAA,82BBBB
py -3.13 tools/recomp-trace/first_pass.py sessions/my_session/trace.tsv SOMEKIND
py -3.13 tools/recomp-trace/bail_impacts.py sessions/my_bail_session/trace.tsv
py -3.13 tools/recomp-trace/collision_posts.py sessions/my_ride_session/trace.tsv --top 6
```

Both refuse a trace with malformed lines of their kinds (record again rather than parse around bad data). Before
comparing collision-sound rates with another engine, filter to the local rider (`local72` = 1): NPC skaters post
into the same manager.

## Example output

`trace.py` (made-up counts):

```
{'MARK': 12, 'PLAY': 5400, 'MOD': 31000, 'GAIN': 18000, 'CAPTURE': 900}
malformed (fixed-layout kinds): {'TREAT': 0, 'SEAMPAT': 0, 'SEAMHIT': 0, 'GRECX': 0, 'FIRSTHIT': 0, 'SKID': 0, 'EMITSLOT': 0}
stop_a                           12.0 ..     95.5 s
```

## Requirements

Python 3.13; `numpy` for `retail_windows.py` and `Trace.capture`. The repository's `tools/owned_game`
and `tools/asset_pipeline/audio_formats.py` for the resolvers.
