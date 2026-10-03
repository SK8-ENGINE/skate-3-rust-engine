# 7. Indexed stock collection lookups (PR E, performance)

## Problem

Loading the skater runtime took ~3.2 s and the camera runtime ~2.5 s, on every
game start and again on every in-game map switch (`map_transition.rs` reloads
`SkaterRuntime`, `PlayerControls` and `CameraRuntime` for the new map; logs
showed `MAP_LOAD_TIMING ... simulation_ms=6224` even for the smallest park).
It also made per-map asset validation cost a full game startup each time.

## Measurement

Timing each step of a map check (instrumentation since removed):

| Load | Time |
|---|---|
| `CameraRuntime::load` | 2.47 s (its `Collections::load` parse: 0.03 s) |
| skater `GroundProfiles::load` | 1.40 s |
| skater `scoring_runtime::Runtime::load` | 1.12 s |
| all other skater steps together | 0.34 s |

## Root cause

`skate_data::collections::Collections::field` (`crates/skate-data/src/collections.rs`)
found each entry by a **linear scan of all ~6,850 collections**, and the match
predicate called `attrib_hash::numeric_name` (string hashing plus `format!`
allocation) on each scanned entry's class and key. The loaders above perform
thousands of `field`/`float`/`words` lookups (`GroundProfiles` alone loads 25
settings tables), so lookups dominated.

## Change

- `Collections` gains a lazily built index `(numeric class, numeric key) → first
  matching position` (`OnceLock<HashMap<..>>`, `#[serde(skip)]`).
- `field` uses the index instead of the scan; the rest of the lookup (field-name
  fallback, inheritance walk, error messages) is unchanged.
- `override_profile` (the only mutation, used by custom difficulty) resets the
  index, so custom tuning changes are never served from a stale index.

### Why results are identical

The old predicate was `(name == x || numeric(name) == numeric(x))` for class and
key. `numeric_name` is a pure function, so equal names have equal numeric names
and the predicate reduces to "numeric names equal". The index keeps the **first**
position per numeric pair, which is exactly what a forward linear scan returns —
including if duplicate entries ever exist (`load` already rejects them; directly
deserialized data might not).

## Verification

- Unit tests (`collections::tests`): the original linear scan is kept as a test
  reference (`field_linear`); spellings (`Hash_…`, readable), inheritance,
  missing/cyclic errors, first-duplicate-wins, and index reset after
  `override_profile` all match.
- Real data (ignored test `index_matches_linear_scan_on_private_collections`,
  `SKATE3_ASSET_ROOT` set): **145,807 lookups over 6,849 entries** return the
  identical field (pointer equality) or identical error.
- End to end: `--validate-maps` results for the test world and all 10 maps
  (collision triangles, spawn support distance, grounded ticks, warnings) are
  identical before and after.
- Private gameplay startup test (`private_extracted_map_supports_production_gameplay_startup`)
  passes on MegaPark and DownTown.
- No new failures in skate-data, skate-core or skate-game test suites (the
  pre-existing upstream failures are listed in PULL-REQUESTS.md).

## Effect

| | Before | After |
|---|---|---|
| Skater runtime load | ~3.2 s | ~0.10 s |
| Camera runtime load | ~2.5 s | ~0.07 s |
| Shared stock loads in `--validate-maps` | 2.56 s | 0.12 s |

Game startup and every map switch save ~5.5 s.

## Files

- `crates/skate-data/src/collections.rs`
