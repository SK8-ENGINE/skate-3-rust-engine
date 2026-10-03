# Planned upstream pull requests

The fork's changes will be offered to SK8-ENGINE/skate-3-rust-engine as separate
pull requests, one per type of change, so each can be reviewed and merged on its
own. **None have been opened yet.** Each PR description is drawn from the linked
document(s), which hold the full context (problem, root cause, evidence,
verification, open questions).

| PR | Type | Title | Documents | Fork commits | Depends on | Status |
|---|---|---|---|---|---|---|
| A | Setup fix | Setup: enable Windows long paths for the pro roster (ISO argument-order half dropped: upstream fixed it in `713fe70`/`7ae67f2`) | [02](02-long-paths.md) ([01](01-iso-extraction.md) superseded upstream) | `0a88f7a`; PR branch `setup/xiso-option-order-and-long-paths` = `4352417` + merge of upstream `7ae67f2` (`d7cd6a2`, upstream lines at both ISO call sites) | — | Draft upstream [#23](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/23), retitled; diff vs upstream `main` = `tools/setup.py` only |
| B | Feature (input) | SDL3 gamepad input with XInput fallback | [03](03-sdl3-gamepad-input.md) | `3d485c8` | — | Draft upstream [#24](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/24) |
| C | Gameplay data | Authored map spawns and headings | [04](04-map-spawns.md) | `b495ed6`, `f00b329` | — | Draft upstream [#27](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/27) |
| D | Gameplay data | Skip collision volumes that have no surface IDs | [05](05-collision-volumes.md) | `dec7ade`, `d68c3ee` (comment: the boxes are retail trigger volumes) | — | Draft upstream [#25](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/25); description corrected 2026-10-02 (SkateSchool box coordinates, see doc 05 "Correction") |
| E | Performance | Indexed stock collection lookups (game startup −5.5 s) | [07](07-collections-index.md) | `810defa` | — | **Merged upstream** as [#26](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/26) (2026-10-02, `4003812`) |
| F | Tooling | Streaming map validator (`--validate-maps`) wired into setup | [06](06-map-validator.md) | `18f41a3` | E (speed only) | Draft upstream [#28](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/28) |
| H | Gameplay + rendering | Water: detection and bail, shallow water solid / deep water floats, board floats, water camera + vignette, entry splash, water look (family 33 cube blur + tint, anti-tiling, body-size calming, slower waves), animated water (frame-state buffer fix, ocean PCA from default.xex, XEX2 unpacker), ripple scale | [09](09-water.md) | `gameplay/water` (`af1b350`…`55776dc`), upstream branch `water/retail-water` (`8a6e328`, one commit on `60efdef`) | — (carries #28's `install.py` `spawn()` split verbatim) | Draft upstream [#30](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/30) |
| I | Test fix | `sky_shader_validates`: add the `VERTEX_POSITIONS` shader def Bevy sets for real meshes | [10](10-sky-shader-test.md) | `b7865d0` | — | Not opened; tiny, can go upstream alone |
| J | Feature (audio) | Game audio: setup group `audio` extracts the disc's audio (vgmstream r2117) and the native data (AEMS banks, MixMap, grains, Splice banks, vault tuning); a native port of the retail audio runtime (`crates/skate-audio`, `crates/skate-audio-fma`) plays all world and player audio; volume settings and `--mute` | [11](11-audio.md), [15](15-world-audio.md) | `gameplay/audio` (latest pushed `870940e`); upstream PR branch `audio/retail-audio` mirrors it exactly (`843cca9`). History: batch commit `f438017` "New audio who dis" (merged as `ca7b4f3`, 2026-10-02), then follow-up commits | H (branched from `gameplay/water`, #30, uses its water detection) and **#35** (50 solver iterations; needed for retail-like pops — #32 carries the same commit until #35 merges). Overlaps upstream #4/#1 (not adopted; doc 11) | Draft upstream [#32](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/32), **Work in Progress**. State 2026-10-03: native runtime is the only path; every retail player component ported; user: "basically retail". Road to ready (`.claude/todo/pr32-ready.md`): code-review fixes (host clock = physics steps, stray files, data tests, fallback) → world / NPC audio built out in #32 with an engine- and mod-facing hook-in surface (P0–P2 done 2026-10-03, uncommitted: map-change fix, `world_audio` components + `sdk.world_audio`, dev mod `mods/world-audio-test`; doc 15; P3–P5 next) → publish the specs, repoint `.claude/` references → moddability pass → second optimisation pass → move the non-audio tools to L → full regression check. #32's description is kept current (standing rule). Commits only when the user asks. |
| G | Performance (setup) | Skip decoding duplicate stream copies; run the customiser beside the maps; parallel clothing library and pro roster, plus a native RefPack DLL in `Build.ps1` | [08](08-setup-performance.md) | `8327e94` (+ overlap hunks of `install.py` in `18f41a3`) | — | Draft upstream [#29](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/29) |
| K | Gameplay fix (physics) | Board/skater solver: 50 constraint iterations (retail live value) instead of `RWMaxIterations` = 25 | [12](12-solver-iterations.md) | `physics/solver-iterations` (`3f6e989`, cherry-picked from `gameplay/audio`'s `7a9e6dc` onto `main` `7ae67f2`) | — (one-file change in `physics/settings.rs`) | Open upstream: [#35](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/35) (2026-10-03). #32 depends on it for retail-like pops and carries the same commit until #35 merges. Open question in the PR: the asset-gated `customiser_equipment_reaches_ground_force_and_torque` test (doc 12). Tests: 309 pass; the 2 failures (`pipelines_accept_…`, `sky_shader_validates`) also fail on `main`. |
| L | Tooling | Development and research tools in `tools/<name>/` (regression checks, setup equivalence, collision / world-stream / audio-file / vault readers, audio e2e and bench; recomp trace and code search, reference only) | [14](14-published-tools.md) | uncommitted (on `gameplay/audio` working tree) | `audio-e2e` / `audio-bench` / `audio-file-inspect` need J's native audio and setup modules; `setup-equivalence/compare_refpack.py` needs G's RefPack DLL; `regression-checks/check_maps.py` uses F's `--validate-maps` (falls back to `--check-assets` without it) | Not opened; the generic folders could go first, the audio ones with or after J |

Opened 2026-10-01 as drafts from per-PR branches cut from upstream `60efdef`
(code only; `docs/hails-additions` stays in the fork). #29 (G) is stacked on
#28 (F); in the upstream branches the customiser-overlap hunks of `install.py`
live in G, and each branch has its own equivalence pairs (F: maps main→F;
G: character/environment/maps from main and from F). Bodies: `.claude/pr-drafts/`.

Notes for whoever opens them:

- Each PR needs its own branch cut from upstream `main`, containing only its
  commits (the fork's `hails-additions` branch carries all of them in sequence).
- PR A overlaps upstream PR #9 (Linux/macOS port), which also fixes the
  extract-xiso argument order. If #9 lands first, drop that half of PR A.
- PRs F and G both change the `maps` fingerprint and both add pairs to
  `tools/asset_pipeline/pipeline-equivalence.json`. The fork currently holds one
  combined pair per group (committed → F+G). When splitting into separate PRs,
  recompute each PR's pairs from its own tree (old = upstream base, new = that
  PR), and re-check the customiser fingerprint is unchanged.
- `install.py` changes for PRs F and G are both in commit `18f41a3` (the
  customiser overlap — `available_memory`, `overlap_customiser`, `Background` —
  is interleaved with the validator wiring in `_install`). Separate them when
  cutting the PR G branch. PR G now changes the customiser fingerprint (one
  rebuild of the customiser, with identical output).
- Upstream sync (2026-10-02, upstream `main` = `7ae67f2`; fork PR [Hailey-Ross/rusty-trucks#4](https://github.com/Hailey-Ross/rusty-trucks/pull/4) syncs our `main`): merging it into `hails-additions`, `gameplay/water` and `gameplay/audio` conflicts only in `install.py` / `customiser_setup.py` (the ISO argument order, same fix both sides — take upstream's lines). `customiser_setup.py` is in the customiser fingerprint, so the next setup refresh after the merge rebuilds the customiser once. `water/retail-water` merges clean. Upstream also created an empty `audio-integration` branch (= `main`), likely for PR #4/#1 audio.
- PR F works without PR E but validates ~10× slower (each skater/camera load
  then takes ~5.5 s instead of ~0.2 s).
- Pre-existing upstream test failures (present on untouched upstream `60efdef`,
  not caused by these PRs): skate-core
  `a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`,
  `predictive_contacts_and_retention_match_full_scan_for_every_primitive`;
  skate-game `production_factory_routes_three_handlers_and_all_five_conditions_to_grind_owner`
  and `embedded_static_rwcm_hits_distinct_actor_query_ids` (both fixed upstream by #18 / #17,
  merged 2026-10-02 — gone once our branches merge upstream `7ae67f2`),
  `pipelines_accept_valid_group_outputs_when_fingerprint_changes`,
  `sky_shader_validates` (fixed on `gameplay/water`, see
  [10](10-sky-shader-test.md)); and three skate-data examples that do not compile
  (`apt_data`, `hud_data`, `scoring_flow_data`).
- PR H includes setup changes (environment group: `ocean_pca.py`, `particles.py`)
  that refresh that group once, and a dev-only mod (`mods/water-test-teleport/`)
  that must be dropped from the upstream branch. The frame-state buffer fix
  (`retail_render.rs` `write_frame_state`) also unfreezes family-14 scrolling and
  the shadow floor, so it could go upstream on its own first.
- Fork-only commits not meant for upstream: `167605a` (ignore the local
  `.claude/` workspace) and `c2be42b` (docs index; the docs travel with each PR).
