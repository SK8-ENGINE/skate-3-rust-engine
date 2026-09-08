# No-complys, footplants and handplants

The stock graphs now reach physical states 602 (no-comply/boneless/fastplant),
601 (air footplant) and 600 (handplant). The implementation connects the TU3
contact selectors, authored animation events, VLT curves, board drives, skeleton
roots, limb IK, release trajectories and state publication. `IsFootPlanting` is
registered in both graphs so its formerly unsupported leaf no longer pauses the
one-foot path.

## Controller controls

These are the existing Xbox mappings and stock graph transitions. Stance and
contact determine which animation and physical state can enter.

| Action | Input |
| --- | --- |
| No-comply | Begin a front-foot push (X in regular, A in goofy), then flick the right stick for an ollie during push entry. The graph accepts the Square gesture group. |
| Boneless / fastplant | Hold one grab trigger fully, then press A or X. The chosen trigger, push foot and stance select the stock variant. |
| Air footplant | Ollie, hold LT or RT for a grab, then press and hold A or X for a one-foot air while approaching a reachable surface. Contact prediction and the extended foot determine entry. |
| Handplant | Hold RB while riding up a steep transition toward reachable coping. Move the right stick to tweak; A/X select one-foot variants and B selects the no-foot variant. |

No-comply launches come from the clip's `FootJump` event. Footplants release at
the manager's calculated curve duration. Handplant entry requires upward travel,
the authored minimum speed/slope, a selected edge and a valid surface
investigation. Holding RB on flat ground does not force a handplant.

## Local build

Run `LaunchPlants.bat` in the repository root. It uses
`bin/plants/skate3rust.exe`, the absolute installed asset directory, and
`--test-world`. Additional command-line arguments are forwarded. The default
test course is useful for no-complys and reachable footplant surfaces; handplants
need transition/coping geometry satisfying the native selector. An installed
converted map can be chosen in the map menu or supplied with `--map`.

Every launcher run now saves both console output streams, build identity and the
exit code to a separate `logs/plants-<timestamp>-<random>.log`. Rust panic
backtraces are enabled. On failure the launcher displays the saved output and
keeps the console open. Earlier launcher runs did not save output, so their
simulation error messages cannot be recovered from this directory.

The RMB/RB crash recorded at tick259 was an explicit Ground error on
`GrabWorld` (Processed2476 bit22). That older placeholder misidentified
`82D38430` as an unsupported Skitch query. The TU3 function actually submits
the handplant candidate via `82D61268`, or resets it via `82D62F20` when the
input is released. Those paths already run through `handplant::ground_query`
and `ground_update`. The obsolete error and its unused duplicate manager have
been removed; input still reaches the stock graphs and handplant selector.

The subsequent normal-map run failed at tick4225 in KnownAir because enabled
foot part15 carried volume group4. This is the existing native
DisableCollisionWithWorld classification, not an unsupported shape. Enabled
group4 shapes now stay in the assembly collision pass but are excluded from
the world query. Re-enabling world collision by restoring group0 takes effect
on the next query. This fixes that explicit exit; the separately logged
off-board world-object query and non-finite-pose failures remain under investigation.

The task executable is a separate copy, built with:

```powershell
$env:CARGO_TARGET_DIR = 'C:/Users/Daddy/Documents/skate3-imported/target'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked --target x86_64-pc-windows-msvc -p skate-game --no-default-features
```

The shared target is retained. Copy its `x86_64-pc-windows-msvc/release/skate3rust.exe`
to `bin/plants/skate3rust.exe` after rebuilding this branch. No installed or
running desktop executable is replaced. The launcher contains a machine-local
asset path; other installations must update that path.

If another worktree has populated this shared target with different workspace
crates, refresh the modification times of `crates/skate-core/src/lib.rs`,
`crates/skate-data/src/lib.rs` and `crates/skate-game/src/main.rs` before building.
That forces the local crates to rebuild without deleting the dependency cache.

## Native provenance

Evidence is the owned TU3 executable image `default_82000000_011B0000.bin`, base
`82000000`, SHA256
`f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4`, generated PPC
instruction listings, stock OnBoard graphs, OnBoard.abin, and skaterschema/VLT.
Registration strings and vtables identify the graph leaves. VMX128 instruction
operands take precedence over unreliable decompiler output and community names.

| Behavior | TU3 evidence |
| --- | --- |
| Input | `8259A554/8259AF60` emits GrabWorld from packed bit28; `8259BB18` emits five handplant controls, named by constructors `82F84F28..82F84F98`. Existing A/X pushes feed the authored no-comply and one-foot paths. |
| No-comply / boneless | State602 vtable `82327434`; entry `82D4C900`, update `82D4C9B8`, launch `82D4CB18/82D4CC18`. Anchors the completed physical toe, uses the stock linear/angular drive modes and four launch curves. |
| Footplant | State601 vtable `82327400`; manager Start `82D704B0`, adjustment `82D70CE0`, update `82D70E40`, publication `82D71040`, nearby-edge refinement `82D6FBB0`. Foot/toe volume setup `82D91330`; postphysics `82D4C6D8`. |
| Handplant contact | Manager `82D61040`; ground query `82D38430`, admission `82D61D88`, position selector `82D63D40`, frame/window/edge clipping `82D640F0..82D65D58`. Uses actual world grind primitives, capped at 40 broadphase candidates. |
| Handplant motion | Launch `82D613B0`; six outgoing queries `82D66318`, selection `82D66948`; arc constructors `82D60770/82D608A0`; COM `82D625C0`, Bezier `82D62408`, orientation `82D62C88`, time warp `82D631A8/82D63280`. Collision position accessor `82D2D988` and landing-normal accessor `82D2D9E8`. |
| Handplant skeleton | State600 vtable `823273CC`; entry/update `82D4C350/82D4C3D8`; reckoning `82D8E1C0`; hand IK `82D633B0`, target clamp `82BD9728/82BD97D0`, upper-body volume suppression `82D91298`. |
| Shared skeleton | Anchored-foot path `82BDEC48`, COM path `82BDF090`, root positioning `82BDEEB8/82BE0E80`. Uses the existing completed animation, physical skeleton, board and four-limb IK owners. |
| Graph timing | `IsFootPlanting` `82BBD888`; preparation `82BBD7F0`; FootPlantAbsorb Update `82BBE4E0`; handplant phase/new-position `82BBDA98/82BBDB98`; antic Begin `82BBE5D0`; handplant scoring Update `82BBF758`. |
| Lifecycle | Footplant release `82D8B970` reads big-endian byte2480 bit0, hence full-word bit24. Common handplant publication `82DB7130..82DB7190`; shared plant wipeout check `82D8FDC0`. Teleport/reset paths clear active plant state. |

Settings are loaded from the installation, using the schema's cached layout:

| Collection class | Used data |
| --- | --- |
| `CCB95A83C78B4FF9/default` | Four PointNegGraphData8 launch curves for state602. |
| `physics_footplantmanager/default` (`172D553937D8E07C`) | Radial speed curve; contact/deck dimensions; leg lengths; Bezier handles; minimum/maximum plant duration; release and wipeout limits. |
| `55B5940075F78842/default` | Seven PointGraph4 query-window curves; depth and vertical drop. |
| `physics_handplantmanager/default` (`0F38084037CB9175`) | Time warp; orientation/heading curves; hand-radius curve; entry/release/IK timings; admission thresholds; apex and outgoing-velocity parameters. |
| `anim_handplant/default` (`F190522E738EC5CB`) | Out/into/antic thresholds at offsets0/4/8. |

The antic blend reads actual clip lengths: `INVERT_HANDPLANT_FS_0_ANTIC` is
0.2 seconds and `INVERT_HANDPLANT_FS_0_ANTIC_LATE` is 1/30 second. FootPlantAbsorb
publishes `absorblength` from the physical manager; no host animation timer or
extra jump impulse selects these launches.

## Validation and boundaries

Release compilation succeeded. Static inspection checked all 32 operation names
in the seven plant templates against source registrations, the five stock
collection classes and point-graph layouts, both antic clip lengths, and the
`FootJump` attributes in the no-comply/boneless/fastplant clips. This is not a
runtime graph-coverage or gameplay-parity result.

The game, launcher, recomp and gameplay harnesses were not executed. In-game
entry/release, contacts, stance combinations, wipeouts, visuals and preservation
of skating/off-board/grind behavior remain for the user's validation.

Queries use the engine's existing synchronous static-world collision and grind
geometry adapters. The outgoing batch is consumed on handplant entry rather
than using TU3's asynchronous job-completion gate and redundant ground-probe
completion dependency. Moving-object coping is not supplied by this adapter.
Scalar vector/trigonometric helpers and quaternion-derived axis-angle rotation
reuse existing engine math; bit-exact Xenon/VMX execution is not claimed.
Handplant scoring updates the existing internal score packet, whose presentation
remains outside this change. Stock assets and private research/build files are
excluded from the commit.
