# Provenance

`abin_importer.py` and `rx2_skeleton.py` are pinned from
`https://github.com/chasmlol/skate-3-trick-toolkit` commit
`85d9d679bc35462ce66fdbba95007b56a94579f0`.

Local correction (2026-09-08): `rx2_skeleton.py` decodes vertex declaration
`0x002C23A5` as big-endian FLOAT2 (format 37), replacing the incorrect SHORT4N
interpretation. This preserves the original secondary UV coordinates used by
hair strand coverage. See `docs/visual-parity-status.md` and the synthetic
`tools/asset_pipeline/test_character_uv.py` regression.

`blender_rx2_abin_export.py` is the project-maintained extension of that
exporter (SHA-256
`58BF997204228E6C25942AFCB7BACE48E19BD478EBBD2E2E8C83695BB8417363`).
It adds a second ABIN source so the established builder can append the exact
OffBoard clips to the same armature and action bank.

They decode animation and skeleton files supplied locally by the user. No
retail payload is included.
