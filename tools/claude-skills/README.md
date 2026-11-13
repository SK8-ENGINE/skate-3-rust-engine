# Claude Code skills for skate3rust

Skills for [Claude Code](https://docs.claude.com/en/docs/claude-code) (an AI coding agent) that capture how this
engine is built, tested, researched and kept retail-accurate. Each skill is a folder with a `SKILL.md` (front matter
`name` + `description`, which the agent uses to decide when to load it) and, where needed, helper scripts in
`tools/`. They are plain Markdown, so they also work as human-readable checklists.

## Install

Skills are loaded from `.claude/skills/<name>/SKILL.md` of the project (the checkout you run Claude Code in) or from
`~/.claude/skills/<name>/SKILL.md` for all projects. The repo's `.gitignore` ignores `/.claude/`, so the shared copies
live here and you install them per checkout.

Copy (Git Bash, from the repo root):
```bash
mkdir -p .claude/skills
cp -r tools/claude-skills/*/ .claude/skills/
```
Or link them so they follow `git pull` (PowerShell, from the repo root; directory junctions need no admin rights):
```powershell
New-Item -ItemType Directory -Force .claude\skills | Out-Null
Get-ChildItem tools\claude-skills -Directory | ForEach-Object {
    New-Item -ItemType Junction -Path ".claude\skills\$($_.Name)" -Target $_.FullName | Out-Null
}
```
Install into the project's `.claude/skills/` rather than `~/.claude/skills/` when you want to use the helper scripts:
several of them locate the repo root from their own path (`<repo>/.claude/skills/<skill>/tools/...`).
Restart Claude Code (or start a new session) after installing; `/skills` lists what it found. Invoke a skill by
asking for the task it describes, or by name (for example `/regression-check`).

## The skills

| Skill | What it is for |
|---|---|
| `build-and-run` | Build (dev and release link configurations), stage into `bin\`, test, smoke-test and launch the game; screenshot mode; audio diagnostics switches. |
| `asset-setup` | Convert your own Skate 3 ISO / extracted disc into assets headlessly, refresh after converter changes, re-point the `assets` junction, diagnose spawns and invisible walls, repair the pro roster. |
| `regression-check` | What to verify after converter, game code, input, water, audio, native dependency or performance changes: every-map spawn and collision baselines, test targets, smoke runs, fingerprint impact, "is this failure mine or upstream's". |
| `vendor-patch` | Vendor and patch a crates.io dependency (Bevy, sdl3-sys) and handle native C/C++ dependencies in both C runtime (CRT) configurations. |
| `make-mod` | Write and validate Lua API 2 mods, including the audio and world-audio mod APIs; moddability rules for engine features. |
| `prior-work-check` | Search upstream PRs/issues, forks and sibling projects before starting work. |
| `conflict-resolution` | Merge branches without losing lines: reuse existing resolutions, hunk-level reuse, a lost-line checker, semantic-conflict detection, stacking conflicting PRs. |
| `optimisation` | Performance work with proof of identical behavior: measure first, byte-identical e2e hashes, an adversarial review checklist, worked examples from the native audio passes. |
| `recomp-research` | Reverse-engineer retail behavior with the skate3recomp research build: find code, write guarded trace hooks, drive the game, analyze traces, port in your own words. |
| `recomp-scripted-runs` | Unattended scripted recomp sessions (pad scripts, teleport routes, background mode, screenshots), passive sessions, the trace line catalog, recomp artefacts to discount. |
| `recomp-audio-trace` | Build the recomp research build (title update, clang toolchain) and trace which audio banks/samples retail plays. |
| `ghidra` | Read retail TU3 (title update 3) functions as Ghidra pseudocode (Ghidra 12.1 + the Xenon360 XEX/XEXP loader with VMX128): where to get the latest releases, setup from your own install, a batch decompile tool, a headless wrapper, an optional language patch for five missing VMX128 instructions, known quirks. |
| `aems-port` | The native Rust port of the retail audio runtime (AEMS evaluator, voice graph, MixMap, grain bed, Splice, player and world components): workflow, oracles, specs, the console-cadence rule. |
| `audio-tuning` | Turn a "this sounds wrong" report into a measured, mechanism-level fix: state logs, headless e2e replays, recomp comparisons, world audio checks, re-exporting audio. |
| `audio-autotest` | The automated in-game check of the audio modding features (muted in the background, or audible for a listener). |
| `living-world` | NPC skaters, pedestrians, traffic and movable objects ported from retail: scope, shared design, multiplayer-ready rules, testing. |
| `retail-menus` | The retail front end on the engine's APT stage: pause menu, Game Settings, Trick Guide, Challenge Map, Extras, Skate Profile, Free Play, popups, the HUD minimap and the loading screen; exporting the menu movies, automated menu screenshots, reading a movie's code. |
| `replay` | The instant replay and replay editor port: what is recorded, the editor model, retail's replay screens, Save to PC export, in-game test switches. |
| `bom-sanitise` | Find and strip byte order marks and other invisible characters (zero-width spaces / joiners, word joiner, soft hyphen) from files, staged changes, commit message files and drafts; report them in recent commits; write text without them in the first place. |
| `setup-bench` | Time a full asset setup and prove a setup / export script change keeps the converted files the same (snapshot, per-stage timings, sha256 manifest, comparison against a second unchanged run). |

## Helper scripts included

| Skill | Scripts |
|---|---|
| `conflict-resolution/tools` | `check_lines_kept.py` (lines a branch added that the merge lost), `resolve_like.py` (resolve hunks exactly like a reference branch), `pick_hunks.py` (resolve conflict hunks by a per-hunk choice) |
| `optimisation/tools` | `e2e_bench.sh`, `compare_runs.sh`, `timing_summary.py` (headless audio bench and identity check), `audio_timing_summary.py` (`AUDIO_TIMING` readout), `frame_log_summary.py` (`SKATE_FRAME_LOG` readout) |
| `recomp-research/tools` | `trace.py` (shared trace reader + malformed-line check), `first_pass.py` (summarize first-pass hooks), `fn.sh` (dump a recompiled function), `fnstrings.py` (strings a function references) |
| `recomp-scripted-runs/tools` | `recomp_script_run.sh` (boot, script, trace, stop), `screen_loop.ps1` (window screenshots), `stop_dev_recomp.ps1`, `make_location_script.py` (Challenge Map teleport scripts) |
| `aems-port/tools` | `e2e/` (scenario writer, render comparisons, frame levels, retail capture windows), `grec_level.py` (retail rolling-bed levels from GREC lines) |
| `audio-tuning/tools` | `check_sets_vs_trace.py`, `long_run_stats.py`, `check_bed_level.py` (world audio vs recomp traces) |
| `audio-autotest/tools` | `audio_mod_autotest.ps1` (the autotest runner) |
| `living-world/tools` | `render_glb.py` (offscreen render of a converted model) |
| `ghidra/tools` | `decompile.ps1` (pseudocode for a batch of addresses), `dump.ps1` (bytes / words / strings at addresses), `find-refs.ps1` (references and raw word matches), `find-strings.ps1` (where a C string sits and who uses it), `apply-names.ps1` (load a function names TSV, default the repo's `tools/ghidra/skate3-tu3` list), `ghidra_headless.ps1` (AnalyzeHeadless without the .bat; `GHIDRA_PRIORITY=Idle` for low priority), `scripts/` (the post-scripts: DecompileAt, DumpAt, FindRefs, FindStrings, FindImm64, ApplyNames) |
| `retail-menus/tools` | `apt_dump.py` (an exported menu movie's actions as text, pool names resolved), `check_trick_clips.sh` (Trick Guide demo clips: names, decode, posture) |
| `bom-sanitise/tools` | `bom_scrub.py` (scan / staged / msg / log / stdin; `--fix` strips, exit code 1 on findings) |
| `setup-bench/tools` | `setup_bench.py` (timed full setup into a scratch base plus a sha256 manifest of the install) |

More general tools referenced by the skills (map/collision inspection, regression checks with baselines, recomp code
search, vault inspection, setup equivalence) are in `tools/` (upstream PR #37, merged).

## Prerequisites

- Windows, Rust (MSVC toolchain), LLVM, CMake (Visual Studio's bundled copy works), Python 3.13 (`py -3.13`), Git
  Bash (for the `.sh` scripts), the GitHub CLI `gh` (for `prior-work-check`). Some scripts use numpy / Pillow.
- **Your own legally owned copy of Skate 3** (Xbox 360). Assets are converted locally by the setup; nothing from the
  game is in this repo or in these skills.
- For the recomp skills: you build [skate3recomp](https://github.com/mchughalex/skate3recomp) yourself from your own
  copy (including the title update the recomp's sources expect), plus research hooks (script player, background
  mode, trace writer and hooks) as described in `recomp-research` and `recomp-scripted-runs`. Credit skate3recomp,
  its rexglue SDK and Xenia whenever you use findings from it. skate3recomp has no license: it is a reference to run
  and read locally; never copy its code or generated code into the engine.
- Audio skills (`aems-port`, `audio-tuning`, `audio-autotest`, the audio parts of `make-mod`) describe the native
  audio engine (upstream PR #32, merged 2026-10-06, and its follow-ups).
- For `ghidra`: [Ghidra](https://github.com/NationalSecurityAgency/ghidra) 12.1.x (NSA, Apache-2.0), a JDK 21 or newer, and
  the [Xenon360](https://github.com/EveryoneEverybody/Xenon360) extension (Apache-2.0) built for the same Ghidra version.
  Its output is a reading aid for your own copy of the game: never copy pseudocode into the engine.

## Principles the skills share

- **Retail parity, proven:** port the real mechanism from the retail code; measurements only validate. No guessed
  levels, weightings or timings; label values with their source.
- **Moddable from the start:** data-driven values (retail values as defaults from setup data), stable identities a
  mod can override, a mod-facing API next to the engine-facing one, cleanup when a mod is disabled.
- **Multiplayer-ready:** stable ids, serializable events, deterministic ticks.
- **Optimize only with proof of identical behavior.**
- **No game code or data in the repo**, and credit every project you learn from.
- **One game instance at a time** (engine or recomp), hard time limits on every automated run, and clean up
  processes and temporary output afterwards.

## Contributing

Keep these copies free of machine-specific paths (use placeholders like `<repo>`, `<recomp checkout>`,
`<extracted disc>`, `<game-install>`, `<ghidra-install>`, `<ghidra-project-dir>`) and personal details. If you change a skill in your `.claude/skills/` copy, bring the generic
part back here.
