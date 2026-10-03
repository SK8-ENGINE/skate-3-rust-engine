# Setup equivalence checks

Prove that a faster setup path produces exactly the same bytes as the code it replaces. Both were
written to verify setup speed-ups (see `docs/hails-additions/08-setup-performance.md`).

| Script | What it proves |
|---|---|
| `compare_streams.py` | A changed district stream loader (`tools/vendor/university/.../skate3_streams.py`) returns exactly what a reference copy returns: every asset's record, source path, offset, stored size and decoded bytes, per district and stream (Pres, Sim, Tex). Also times both. |
| `compare_refpack.py` | The native RefPack DLL (`target/native/refpack.dll`, built by `scripts\Build.ps1`) decodes exactly like the pure-Python decoder: every entry of `createacharacter.big` and `marquee.big`, and with `--streams` every extracted district stream asset. |

## Inputs

- Your extracted disc: `--disc DIR` (the folder holding `data/`), default `$SKATE3_DISC` or `.local/skate3-disc`.
- Extracted districts under `--work` (default `.local/collision`), made by
  `tools/collision-inspect/map_collision.py` or `scan_surfaceless.py`.
- For `compare_streams.py`, a reference copy of the loader:
  `git show <commit>:tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py > .local/ref.py`

## Usage

```
py -3.13 tools/setup-equivalence/compare_streams.py .local/ref.py MegaPark StartPark
py -3.13 tools/setup-equivalence/compare_refpack.py --streams
```

Both exit 1 on any difference.

## Example output

(made-up timings)

```
MegaPark             Pres assets   812  old    1.90s  new    0.85s  IDENTICAL
MegaPark             Sim  assets   405  old    0.70s  new    0.31s  IDENTICAL
TOTAL old 2.6s new 1.2s
```

## Requirements

Python 3.13 (standard library); the repository's setup modules (`tools/asset_pipeline`,
`tools/owned_game`, the vendored map tools). `compare_refpack.py` needs the DLL from `scripts\Build.ps1`.
