# Kimodo 360-flip body replacement

The regular 360 flip uses Kimodo variation 1 from the three-pose Blender test.
The existing `B_360FLIP_G` / `B_360FLIP_A` graph path, metadata, timing,
stance mirroring, trajectory and skateboard animation still own the trick.
All D/GONZ/HSU low/high ground and air slots receive the replacement body.
Nollie variants, dark catches, idle and carving are unchanged.

The optional local asset is `assets/private/custom/360flip.json`. Missing it
restores stock playback; malformed data reports an error at startup. The asset
is private and ignored by Git, like the other imported animation data.

The file contains 46 absolute native-space skeleton poses converted from the
Blender comparison's `KIMODO Kickflip - Variation 1` action. Its translation
is anchored to the source clip's first hip position; the comparison's side-by-side
object offset and Kimodo's floor normalization are removed. The source ground
section is frames 0–12 and the air section is 13–45, independently retimed to
each stock slot's original key count. Three-frame entry/exit fades retain the
stock endpoints. There is no new fade at the ground/air seam.

At load time the evaluator converts body globals into local deltas relative to
the bank's `RIG_TPOSE`. Board-parented hand/toe targets are recomputed against
each stock frame's moving board. Trajectory and board subtree samples, channel
weights, loop transforms and headers remain the original values. All normal
tree blending and mirroring then operate on those samples.

Source workspace: `../skate-animation-extraction/kimodo-three-pose-test/`.
`export_game.py` is the Blender CLI exporter; `generation-info.json` records the
Kimodo model, seed and prompt. Only Skate 3 source frames 1, 23.5 and 46 were
provided to Kimodo. Its output is native 30 fps, retargeted to this skater and
resampled to the source clip's 60 fps timeline for this replacement.

Export SHA-256:
`f086c70fabf13f02f59f7b792f528d7c519dad43c7536c168e675f1069580294`.

Validation covers native reference-pose removal, malformed assets, all twelve
replacement slots, unchanged board/trajectory samples and timing, entry/exit
poses, channel weights, finite mirrored poses and unchanged nollie/idle clips.
The original 360-flip graph pipeline fixture also passes in both stances.
Rendered evaluator captures check the imported body and retained board together;
these are not a controller-played visual acceptance test.
