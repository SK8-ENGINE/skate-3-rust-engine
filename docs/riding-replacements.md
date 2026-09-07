# Newer Skate riding poses

Optional `assets/private/custom/riding.json` replaces 33 clip leaves inside the normal native playback tree: three `R_IDLE_HCOM_*` idle/lean clips and thirty `MED`, `ML`, and `LNG_CRV_POSE_*` carving clips. Missing the file restores stock sampling. Invalid files fail with a descriptive load error.

The host substitutes retargeted body channels before the existing blend, reference-pose composition, and stance mirror commands. Original clip timing is mapped by normalized phase onto the replacement samples. The stock trajectory, board/truck/wheel channels and channel weights are preserved. Pushes, tricks, crouching, speed tuck, offboard motion and the graph's controller parameters are unchanged.

Sources from the installed newer Skate build are `c_proto_onb_idle_rolling_slow_neutral_pose`, `fast_neutral_pose`, `fs_pose`, `bs_pose`, `fs_fast_pose` and `bs_fast_pose`, all with the same `c_proto_onb_idle_rolling_` prefix. Neutral idle includes the 241-sample `neutral_addi` motion composed onto its base pose. Carving uses the corresponding frontside/backside poses, with faster poses for MED, a halfway blend for ML and regular poses for LNG. The native ANGLE children retain their frontside/backside ordering. This adapts those clips to Skate 3's graph; it does not reproduce the newer game's runtime graph or physics.

The retarget uses the actual GLB bind skeleton and native bone axes. Both leg chains are fitted to the existing deck foot positions; foot/toe orientation stays matched to the board. The result is converted to local animation samples relative to the stock RIG_TPOSE, so the normal reference composition still applies exactly once.

Extraction, retargeting, numerical bake, previews and reports are outside the project at `C:/Users/Daddy/Documents/skate-animation-extraction/riding`. `retarget_riding.py` is in its parent folder. Run it through Blender from that parent directory, then run `riding/bake_replacements.py` with Python to rebuild the private runtime asset.

Validation: `cargo test -p skate-game riding_replacements -- --include-ignored` with `SKATE3_ASSET_ROOT` set checks all 33 replacements at four phases, original board/trajectory and channel weights, and untouched crouch/push/tuck clips. The real-asset `stock_skater_startup` test exercises the shared physics pipeline. Source and retarget reports, foot fit errors and render captures are retained beside the bake tools.
