# CPU material binding experiment

The user reports poor FPS while looking into University and good FPS looking out
of the map, still CPU bound. The map being resident does not establish that every
triangle is drawn. The captured CPU render work is the actionable bottleneck.

The v4 audit capture `university-audit-7802-28261.json` recorded 4,918 frames in
20 seconds without dropped trace events. Mean command generation was 1.782 ms per
frame; the main opaque encoder finish accounted for 0.995 ms, versus 0.349 ms in
opaque draw traversal. The v5 shadow batch capture
`university-shadow-batches-12344-4318.json` recorded 3,309 frames without dropped
events. Main encoder finish remained 1.012 ms per frame. Shadow visibility fell
from 0.466 to 0.349 ms, but that did not address the dominant main-pass cost.
These are different workloads, not a controlled performance comparison.

A discarded CPU wall-occlusion prototype split at most 256 coarse batches and
used conservative whole-AABB occlusion against real opaque wall triangles. Its
static inward view culled only 55 of 3,512 candidates while costing about 1.15 ms.
It was removed before shipping; adding that CPU cost was unjustified.

## Implementation

RetailWorldMaterial now uses Bevy's bindless material allocator, with a resource
limit of 64 per slab. Materials share texture/sampler arrays and a packed parameter
buffer. The existing shared frame/shadow state remains a buffer reference. This
reduces distinct material binding groups and allows compatible meshes to share
multidraw batches. Pipeline, mesh allocation and resource capacity still constrain
batching; this is not a promise of a 64-fold draw reduction.

The main and depth/shadow shaders use the material slot stored in Bevy's mesh
instance data. Both retain the original bindings when Bevy reports that bindless
resources are unsupported. The common shader module is permanently loaded using
Bevy's shader-library macro so custom imports resolve at runtime. The shading
body, alpha rejection, geometry and texture content are preserved. No user
graphics setting, native physics, input or simulation cadence changes.

The unsuccessful v5 extra shadow meshes and their light-layer adjustment are
removed from the active renderer. Normal world meshes cast shadows again. The
data-only shadow experiment remains test-only for historical reproducibility.

Trace configuration includes `retail_bindless`: true means the device supports
the selected material-array path, false means fallback, null means device data
was unavailable. This indicates capability selection, not measured draw counts.

## Validation and acceptance

GPU-free shader tests compose the actual material shaders and real Bevy vertex
interfaces, using fixtures for unrelated lighting resources/functions. They
validate six combinations: main, shadow depth and depth with normal/motion output,
each with bindless enabled and disabled. A separate test checks the Rust-generated
resource index table and buffer bindings against the shader contract.

The release shader tests passed (both tests, six shader variants). Eleven world
adapter tests and two scene lifecycle tests passed; the offline map inventory
test remains ignored in this ordinary regression run.

Runtime pipeline creation, appearance and FPS remain unverified until the user
runs the private executable. The agent does not launch gameplay. Compare v4 and
v6 from the same stationary inward-facing view with identical settings, then the
same route. Inspect `retail_bindless`, main opaque encoder finish, total command
generation and frame percentiles. Keep the earlier executables for rollback.
