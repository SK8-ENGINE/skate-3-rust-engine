# CPU follow-up after v8

The user confirmed substantially higher FPS in v8 and authorized all three next
targets. The new 20-second v8 capture completed without dropped events and confirmed
bindless materials enabled. Inclusive CPU scopes measured main opaque encoder
finish at 1.47 ms/frame, directional shadow visibility at 0.52 ms/frame, and
controller polling at 0.39 ms/frame. These costs overlap and are not promised savings.

## Resource reuse in material groups

The vendored Bevy allocator now chooses the fitting slab requiring the fewest new
resources, preferring an already fuller slab on ties. Previously it chose the
first fitting slab, even if a later slab already contained all the textures.
This targets duplicate texture references and the associated render-command
resource tracking. Capacity remains 64 for retail materials; shader code is
unchanged. Allocation, reference counting, retirement and GPU uploads retain the
existing implementation. Selection performs more work when materials are loaded
or replaced, not on steady-state rendered frames. An FPS benefit still requires
a matched capture; this is not a guarantee of fewer total slabs on every workload.

The explicit Vulkan test exercises the real allocator with GPU texture identities:
fill two slabs, free the first, reuse the second's resident textures instead of
duplicating them into the first, then retire and reallocate the resources.

## Static shadow spatial index

Retail map meshes receive a static-shadow marker. A cached binary bounding-volume
hierarchy rejects groups outside a directional shadow frustum. Each leaf still
uses Bevy's original per-mesh OBB test with near-plane rejection disabled and far
rejection enabled. Node bounds conservatively enclose transformed mesh bounds;
node render-layer unions also reject unrelated lights early.

The index rebuilds when relevant transforms, bounds, inherited visibility, render
layers or membership change. Camera movement changes queries, not the static tree.
Unmarked/dynamic meshes, missing bounds/transforms, explicit visibility ranges and
NoFrustumCulling use the ordinary per-mesh semantics. Shadow visibility is still
marked through deferred commands, retaining ordering relative to point lights and
newly hidden entities. No geometry, shadow distance, cascade count or visuals change.

Tests compare results with upstream Bevy across mesh movement, visibility and layer
changes, exclusion removal, disabled lights and entity retirement; another compares
the tree against flat OBB checks over 4,096 rotated/scaled boxes. Schedule replacement
is tested separately. A data-only University benchmark uses real material bounds
and representative shadow frusta; its result is not whole-frame performance.

## Controller metadata cache

XInputGetState still runs every host frame for every eligible slot. Raw packets,
packet-number handling, fixed-tick publication and simulation cadence are unchanged.
Only the controller subtype obtained from XInputGetCapabilities is cached. State
errors/disconnects and filtered-out slots invalidate it immediately. Metadata is
also refreshed once per second so a hot swap cannot retain a subtype indefinitely
if Windows never reports a disconnect. Failed refreshes remain errors and are
retried, never stored as successful data. Menu-only polling retains its original API.

This removes redundant capability queries; the 0.39 ms total polling cost also
includes state queries, so it is not all recoverable by this change.

## Verification scope

Headless Vulkan main/shadow tests retain the v8 pipeline-crash regression and 8x
MSAA pixel readback. The game and native gameplay are never launched by the agent.
Existing v8 and recovery binaries remain available. Final frame-time acceptance
requires the same scene/settings and route in the private v9 build and v8.

Final targeted checks passed: 47 ordinary tests (29 input, 3 shadow index/schedule,
11 world adapter, 2 scene lifecycle, 2 shader/descriptor), plus both GPU probes in
bindless and fallback modes. The real allocator reuse/retirement assertions run
inside those GPU probes. The explicit University inventory also passed geometry
preservation and indexed-versus-flat visibility equality.

For 4,840 static retail batches and 1,200 representative shadow queries, the final
single-thread kernel benchmark measured 22.1027 ms flat versus 4.1048 ms indexed
(81.4% lower query time), with a 1.8785 ms index build. It excludes ECS refresh,
dynamic casters, deferred visibility writes and the upstream parallel scheduler;
it is not an 81.4% reduction in whole shadow-system time or frame time.
