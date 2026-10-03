# Hails' additions

Changes made in the [Hailey-Ross fork](https://github.com/Hailey-Ross/rusty-trucks)
of SK8-ENGINE/skate-3-rust-engine, documented so upstream maintainers have the
full context when reviewing a pull request: what broke, why, what changed, how
it was verified, and what is still open.

Each document stands alone. File paths are relative to the repository root.

| # | Change | Area | Status |
|---|---|---|---|
| 1 | [ISO extraction argument order](01-iso-extraction.md) | Setup (Python) | Superseded: upstream fixed it the same way (`713fe70`, `7ae67f2`, 2026-10-02); dropped from PR A |
| 2 | [Windows long paths during setup](02-long-paths.md) | Setup (Python) | Done; UAC prompt and frozen relaunch tested end to end (2026-10-02) |
| 3 | [SDL3 gamepad input](03-sdl3-gamepad-input.md) | Game (Rust), build scripts, vendored `sdl3-sys` | Done, verified in play with an Xbox Elite Series 2 |
| 4 | [Authored map spawns](04-map-spawns.md) | Asset pipeline (Python) | Done, all 10 maps confirmed in play |
| 5 | [Invisible collision volumes](05-collision-volumes.md) | Game data loader (Rust) | Done, all four affected maps confirmed in play |
| 9 | [Water](09-water.md) | Game physics, camera and rendering (Rust), setup, diagnostics | Matched to retail footage (RPCS3): shallow water solid, deep water floats, board floats, water camera + vignette, entry splash, canal water look matched, small bodies calmer |
| 10 | [Sky shader validation test](10-sky-shader-test.md) | Game tests (Rust) | Done; all six shader validation tests pass |
| 11 | [Game audio](11-audio.md) | Setup (Python, vgmstream), game audio (Rust) | Work in progress (draft #32). Native port of Skate 3's retail audio runtime (`crates/skate-audio`): AEMS evaluator, voice graph, MixMap, granular rolling bed, Splice, buses, every player component; the only audio path since 2026-10-03 (interim cue tables removed). Retail world audio: .ems emitters, location-set one-shots, zone ambience. User: "basically retail". |
| 12 | [Board solver: 50 constraint iterations](12-solver-iterations.md) | Game physics (Rust) | Done; maps identical, water traces as expected; one asset-gated customiser test fails (open question) |
| 13 | [Research hooks for the recompilation](13-recomp-research-hooks.md) | Research tooling (external: skate3recomp `research-hooks` branch) | Published; reference and information gathering only (the recompilation is not a perfect copy of the console game) |
| 14 | [Published development and research tools](14-published-tools.md) | Tooling (`tools/<name>/`, Python and shell) | Done (uncommitted); scripts compile and print their usage; the two `recomp-*` folders are for the recomp's research hooks, reference only |
| 15 | [Hooking up world audio](15-world-audio.md) | Game audio (Rust), mod API (Lua) | Part of #32 (uncommitted). Engine-facing components (`TrafficAudio`, `PedAudio`, `NpcSkaterAudio`) and `sdk.world_audio` for mods feed the ported retail traffic / ped / NPC-skater audio with retail's limits; the map-change bug fixed; a dev test mod (cars, peds, a ghost skater) makes it audible now |

## Environment used for verification

- Windows 11, Intel i7-14700KF, NVIDIA RTX 4080 SUPER (Vulkan).
- Rust 1.98.1 (MSVC), Visual Studio 2026 C++ tools, Windows SDK 10.0.26100, LLVM 23.1.2, Python 3.13.15.
- Skate 3 Xbox 360 disc image (103 files, 6,404,940,920 bytes).
- Xbox Elite Series 2 controller over Bluetooth LE.

## Conventions

- Upstream behaviour is preserved unless a document says otherwise; where a
  behaviour changes on purpose (for example, a map's default spawn), the
  document lists the old and new values.
- Project choices that are not dictated by retail data are called out as
  such, so maintainers can overrule them.
