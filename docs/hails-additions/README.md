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
| 11 | [Game audio](11-audio.md) | Setup (Python, vgmstream), game audio (Rust) | Upstream #32 (ready for review; since 2026-10-04 the one audio PR, also carrying the former #36, #43, #44 and #49). Native port of Skate 3's retail audio runtime (`crates/skate-audio`): AEMS evaluator, voice graph, MixMap, granular rolling bed, Splice, buses, every player component; the only audio path since 2026-10-03 (interim cue tables removed). Retail world audio: .ems emitters, location-set one-shots, zone ambience. User: "basically retail". |
| 12 | [Board solver: 50 constraint iterations](12-solver-iterations.md) | Game physics (Rust) | Done; maps identical, water traces as expected; one asset-gated customiser test fails (open question) |
| 13 | [Research hooks for the recompilation](13-recomp-research-hooks.md) | Research tooling (external: skate3recomp `research-hooks` branch) | Published; reference and information gathering only (the recompilation is not a perfect copy of the console game) |
| 14 | [Published development and research tools](14-published-tools.md) | Tooling (`tools/<name>/`, Python and shell) | Done (uncommitted); scripts compile and print their usage; the two `recomp-*` folders are for the recomp's research hooks, reference only |
| 15 | [Hooking up world audio](15-world-audio.md) | Game audio (Rust), mod API (Lua) | Part of #32. Engine-facing components (`TrafficAudio`, `PedAudio`, `NpcSkaterAudio`) and `sdk.world_audio` for mods feed the ported retail traffic / ped / NPC-skater audio with retail's limits; the map-change bug fixed; a dev test mod (cars, peds, a ghost skater) makes it audible now. Session marker sounds (branch `audio/respawn-marker`, former #49, now in #32): the session marker's retail sounds (front-end `fe` records, `sk8_menu`; the hold's Treatments teleport crackle), `ui_audio` events, `sdk.audio.frontend` / `teleport_effect`. Car alarm trigger (2026-10-04, former #44, now in #32): retail's parked-car rule as `VehicleImpact` / `VehicleParked` / `CarAlarmRule`, mod event `impact`, `alarm_rule` |
| - | [Audio specs](audio-specs/README.md) | Reference for 11 and 15 (Markdown) | Published 2026-10-03: the behavioural specs (AEMS evaluator, voice graph, buses, MixMap, grain player, player components, world / NPC audio, prior work) that docs 11 / 15 and the code comments cite by section |
| 16 | [Audio modding](16-audio-modding.md) | Mods (Lua API 2), native audio (Rust) | Part of #32 (the former #36 and #43): the modder's guide (`audio.json` content overlay, custom-map audio by sidecar / `.skate` tag / mods, retail posts / globals / MixMap watch, observe-only audio events, limits, `check_mod`), the implementation and its byte-identical proofs, then the design and research. Depends on #32. |
| 25 | [Performance diagnostics and setup phases 4-5](25-perf-diagnostics.md) | Game (Rust), mod API (Lua), setup (Python) | Upstream #51; merged into `hails-additions` 2026-10-04 (frame-time row id 27 there). Frame-time counter and log, `sdk.snapshot.frame`, below-normal setup priority and budget overrides, threaded `write_map`, props beside collision and `write_map`; setup outputs byte-identical; `FRAME_HITCH` log line (2026-10-08); follow-up `ui/frame-counter-modes`: counter Off / Simple / Verbose |
| 26 | [Living world: NPC skaters, pedestrians and traffic](26-living-world.md) | Setup (Python), data (Rust), game (Rust), mod API (Lua) | Upstream draft #52; scope frozen 2026-10-10 (finish: play session fixes, final checks, ready on the user's word; later work in the follow-ups list). In: the `livingworld` setup group, the population core (seeded, multiplayer-ready), NPC skaters (retail lines, trick choice, steering, avoider, opt-in simulated tier, mode 7), pedestrians (models, navmesh, behaviour graph, moods, reactions, chases, takedowns, tazers, greets, conversations, plugins on benches / bins / vending machines, hand props with carry poses, the attack throw), traffic (census, signals, horn, manoeuvres, car hits, car alarm, opt-in skitching), zombie mode (peds, empty streets, yellow screen; `SKATE_ZOMBIE=1`). Sub files 26a to 26i by topic (index in doc 26); designs in [`living-world/`](living-world/) |
| 27 | [Dynamic props (movable DMOs) from upstream PR #15](27-dynamic-props.md) | Setup (Python), data (Rust), game (Rust) | Part of #52 (laaledesiempre's #15 cherry-picked with authorship kept). Retail contact solver and sleep, per-type DMO data, the props' retail shader, Reset / Upright, props created mid-game, Move Object (retail carry), DMO streaming (census core and in game, on by default as retail) |
| 30 | [Grinds: random bails at some rails](30-grind-random-bails.md) | Game physics (Rust), diagnostics | Merged upstream #57 (2026-10-09), branch `fix/grind-random-bails`: ports retail's per-volume world-query box (82777E70) and its per-triangle test (8277BC58), so the deck no longer gets a predictive contact with the post below the library handrail bend; the rail-corner bails (real wheel hits) stay open |
| 32 | [See-through fences and grates: retail `environment.transparent` shading](32-transparent-fences.md) | Game rendering (Rust, WGSL), setup (Python) | Merged upstream #58 (2026-10-09), branch `fix/transparent-materials`: fences drew as opaque black panels (shader fell back to family 1); new family 16 ports `transparentenvironment_defaultPS` (alpha-scaled lightmapped diffuse, alpha squared out); in-game check pending |
| 31 | [Retail menus, milestones 1 and 2](31-retail-menus.md) | Setup extraction, game data (Rust), mod API (Lua) | Milestones 1 to 4 merged upstream #59 (2026-10-09), branch `ui/retail-menus`. Retail menu tables (settings rows and screens, crossbar categories and items, Apt keys, per-mode menus) extracted from the user's own executable at setup by content and code pattern; `MenuRegistry` with visible / enabled / select / highlight / value hooks, every retail entry greyed until handled, retail rules as data; `sdk.menus` (add, hide, reorder, relabel, handle, values; reverted on disable). Milestone 2: retail Left / Right value rules per settings row (volume slider 0.1 steps with snap, toggles, Play Mode wrap), SFX volume, Camera Angle and Play Mode bound to our settings, mod value / rule overrides. Nothing drawn yet |
| 33 | [Rolling on grass and dirt](33-grass-rolling-sound.md) | Game audio (Rust) | Fix, branch `fix/grass-rolling`: the grain bed's stand-in grains on the Class_rolling surfaces (grass, dirt, tags 8 / 10 / 70 and 37, 67..69) replaced by retail's routing (`sub_824C5CA8`: no grain there), one binding path for both routes, every tag 0..127 pinned by a parity test. The Ghetto Spot grass report is retail parity: retail plays the same SpiderCracks layer (seam pattern 1) there, shown by retail code and collision data and confirmed in the recomp |

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
