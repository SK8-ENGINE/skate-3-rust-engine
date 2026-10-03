# 6. Streaming map validator (PR F, tooling)

## Problem

Setup validated every converted map by launching the game once per map
(`skate3rust --assets <stage> --map <map> --check-assets`, inside each
`map_job` subprocess), plus a `--test-world --check-assets` launch before and
after the map stage and one per recovered old map. Each launch cost ~7–8 s, of
which only 0.1–2 s depended on the map: ~6 s was the same stock loading
(skater runtime 3.2 s, controls + camera 2.5 s) repeated every time.

The check was also weak: a map passed as long as it loaded. It did not catch the
spawn problems found in play (skater falling into the void on MegaPark, standing
on an invisible collision-only floor in Industrial).

## Change

### Game: `skate3rust --assets <dir> --validate-maps`

New module `crates/skate-game/src/map_validation.rs`, dispatched from `main.rs`
after the stock loads (config, gameplay config, manifest, graphs):

- Loads once: animation source, controls and camera (fail fast, like
  `--check-assets`). Prints `SKATE_VALIDATOR_READY seconds=…` on stdout.
- Reads one request per stdin line (a `.skate` path or `TEST_WORLD`) and answers
  with one `SKATE_MAP_CHECK {json}` stdout line; exits 0 at EOF. All other game
  output stays on stderr. Works through the crash supervisor, which forwards the
  child's stdout/stderr unchanged and lets it inherit stdin.
- Per request:
  - **errors** (the map is rejected, as before): map load,
    `skate_world::validate_runtime`, physics with the map's collision, skater
    runtime. A panic inside one map becomes that map's error (`catch_unwind`).
  - **warnings** (reported, never rejecting): from
    `physics/startup_check.rs` — no collision within 10 m below the spawn; deck
    spawned away from the map spawn/heading; a neutral startup (zero actions,
    as with no controller) must settle with wheel support for 12 consecutive
    ticks within 240 ticks (4 s at 60 Hz), with finite body state, no physics
    failure and a solved wheel contact; and the collision floor under the spawn
    must have a visible (render) surface within 1 m.
  - JSON fields: `path, ok, errors, warnings, collision_triangles, spawn,
    support_drop, grounded_ticks, visible_floor_gap, seconds, timings`.
- `--check-assets` is unchanged.

### Setup (`tools/asset_pipeline/install.py`)

- `MapValidator` runs one validator process; `start_validator` falls back to
  per-map `--check-assets` launches if it cannot start (e.g. an older game exe),
  and `validate()` falls back for the rest of the run if the process dies.
- The pre-map stock check, every map (as soon as its conversion finishes, while
  the remaining maps keep converting), recovered old maps and the final
  test-world check all go to the same process. The per-map launch was removed
  from `convert_map`.
- Process launching (packaged-exe DLL/PATH handling) moved from `run()` into a
  shared `spawn()`; `run()` behaves as before.
- Warnings are written to `assets/private/map-status/<map>-validation.json`
  (`status: "warning"`) by `record_validation`. New module
  `validation_report.py` wraps `optional_content.summary` and adds them to
  `setup-report.json` and the setup message; `install.py` and `tools/setup.py`
  call the wrapper. `optional_content.py` itself is deliberately untouched: it is
  part of the hud/environment/maps fingerprints **and** of every
  character-customiser stage version (`customiser_cache.SHARED`), so editing it
  would make every existing installation rebuild the customiser (~4 min), which
  has no equivalence mechanism. (A first version edited it; the fingerprint
  check that caught this compares against the committed tree, not a local
  install.)
- `pipeline-equivalence.json`: removing validation from `convert_map` changes the
  `maps` fingerprint without changing any output, so an old→new pair is added
  (combined with PR G's stream change, see doc 8). Verified: an installation
  built from the committed code rebuilds no group, and the customiser
  fingerprint and all five stage versions are unchanged.

## Verification

- Validator on the test world and all 10 maps: all `ok`, no warnings; collision
  triangle counts equal the post-volume-fix baseline.
- Negative cases (bad spawns written into copies of real maps):
  MegaPark's old void spawn → "no collision within 10 m below the spawn" +
  "wheels never found support"; Industrial's old harbour-bed spawn → "no visible
  surface … (invisible collision)".
- Known limit: a spawn on a solid, visible roof (Industrial Skate Park's old
  roof spawn) is not flagged — rooftops are legitimate skate spots; only
  authored start data (doc 4) distinguishes them.
- Unit tests: `map_validation::tests` (visible-surface heights, single-line JSON
  with the documented fields); Python `test_map_validator.py` with a fake
  validator speaking the protocol (results, warnings into `setup-report.json`,
  start failure fallback, process death, request injection, pipes closed under
  `-W error::ResourceWarning`). Existing setup/pipeline suites pass (40 tests).
- Fingerprints: with the equivalence pairs, an existing installation rebuilds
  no groups.
- Full setup with the real game: see "Setup run" below.

### Setup run

Full fresh setup from the extracted disc with the real game exe (Windows 11,
i7-14700KF, 3 map workers): **466 s, 0 warnings**; the validator was used
throughout (no fallback).

| Step | Before | After |
|---|---|---|
| Pre-map stock check (`--test-world --check-assets` → validator start + `TEST_WORLD`) | ~6 s | ~1 s |
| Per-map validation | ~7–9 s each, inside each map job | 0.28–1.79 s each, in the parent while other maps convert |
| Map stage (convert + validate 10 maps) | ~217 s | 183 s |

Per-map `phase_seconds.validate` in `maps.json`: University 1.54, DownTown 1.79,
Industrial 1.11, parks 0.28–0.37 s. Afterwards `check_maps`-style validation of
all maps: all ok, no warnings, collision triangles identical; baked spawns
identical to the authored values (doc 4).

## Notes

- The ignored private test `private_extracted_map_supports_production_gameplay_startup`
  uses a fixed 24-tick window and fails on Industrial Skate Park, whose authored
  start is 0.75 m above the floor (≈0.39 s ≈ 23–24 ticks of fall). The map is
  fine in play; the validator uses the settle rule above instead. The test was
  left unchanged.
- Without PR E (collection index) every per-map check would still take ~5.8 s.

## Files

- `crates/skate-game/src/map_validation.rs` (new), `physics/startup_check.rs` (new),
  `main.rs`, `config.rs`, `physics.rs`, `skater_animation.rs` (`AnimationSource::load` visibility),
  `retail_character.rs` + `tests/map_transition.rs` (new `Config` field in test literals)
- `tools/asset_pipeline/install.py`, `validation_report.py` (new), `pipeline-equivalence.json`,
  `test_map_validator.py` (new), `tools/setup.py` (message wording)
