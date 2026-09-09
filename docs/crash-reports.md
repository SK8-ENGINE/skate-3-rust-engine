# Crash reports

Both game executables supervise a child process before loading assets or Bevy. On a nonzero exit (startup error, Rust panic or native process failure), the surviving supervisor saves a UTF-8 report in `%LOCALAPPDATA%/Skate3RustEngine/CrashReports`, falling back to the temporary directory. The small Windows popup provides **Copy report**, **Open folder** and a reviewable text preview. Nothing is uploaded. Reports persist until the player deletes them.

The report records embedded build provenance, process status (including Windows exception exit codes), Rust panic backtraces where available, bounded recent logs, GPU/driver details after renderer startup, map content fingerprint, difficulty, render settings and sampled gameplay/multiplayer state (provider, remote count and RTT, without peer identifiers). Missing information is labelled. The map fingerprint identifies content without sharing a personal filename. Mod contents, lobby IDs, player names, addresses and authentication details are not deliberately collected. Sensitive-context and absolute-path log lines are omitted; review reports before sharing. Existing launcher logs retain their original behaviour and are **not** copied wholesale into reports.

The supervisor retains 256 log lines, 64 recent transitions, 128 panic/native exception lines and six metadata records, each bounded to 4 KiB. Gameplay samples once a second and drains a fixed 64-entry queue of physics-state requests; recording those requests only copies scalars. It does not walk stacks, flush files or enumerate hardware each frame. Pending requests and short menu/map transitions may be missed at failure. Reports are assembled and written only after failure. Pipe readers have bounded line buffers, including for unterminated output; the supervisor allows two seconds for final output after exit.

Coverage limits: a Windows unhandled-exception filter attempts a fixed, allocation-free pipe write of exception code, fault address and executable image base; another handler, fail-fast or severe corruption may bypass it. No native fault-thread stack/register capture or memory dumps; native exit codes alone do not prove a particular exception occurred. A panic hook is best effort under OOM, stack overflow or runtime corruption. A successful exit, forced termination of the supervisor/process tree, missing executable dependencies, power loss, an OS failure or an unusable desktop/GPU may prevent a popup. A hung game is not forcibly killed. PowerShell/WinForms provides the full UI; if startup fails, a native message box gives the saved report location. A full/unwritable disk can prevent saving.

`Build-Release.ps1` links the release directly into fresh private staging and retains a matching EXE/PDB pair plus hashes and revision in the sibling `symbols` folder, outside the player ZIP. Keep that folder with each release for offline symbol matching; it contains no original Skate assets. Release line tables are enabled and stripping is disabled. Dirty builds are explicitly marked and carry a unique build timestamp; release from a clean commit for reproducible source matching. Development launches (`cargo run`, `PLAY.bat`, multiplayer and packaged executables) use the same supervisor automatically.

For manual UI validation, run the executable with `--crash-report-preview`. This generates a clearly synthetic report and opens the popup without initializing the game, setup, Steam or assets. Check Copy report, clipboard text, Open folder, saved UTF-8 text and Close. Static tests do not validate desktop presentation.

## SetDeckPitchAndYaw (2026-09-08)

The supplied report stopped in KnownAir at tick4813, MotionGraph behavior3373:
`SetDeckPitchAndYaw has no recovered native implementation`. This was an
unimplemented graph behavior, not an animation overflow.

TU3 factory82BC7B20 retains the authored `skateyaw`/`skatepitch` attribute names
(defaulting to those same strings). Vtable8231FF24 calls Begin82BAF100, which
copies completed Skeleton PhysOut536 then540 to scalar animation parameters.
Update and End both point to empty82B61BB8. Skate2 constructor82CB03E8 and
Begin82CB04D0 corroborate the same behavior and output offsets.

The connected producer is Skeleton::Fill82BE1AE8: read Skeleton6512
(animation-record pose0 column2), negate it when animation packet10368 is
nonzero, calculate yaw from X/Z with native atan82473B98 and quadrant/zero
selection, and calculate pitch as DOT3 with up82139A20. Pitch is the signed
forward-Y projection, not an Euler angle or asin. VMX estimate bit identity is
not claimed.

`SkeletonOutputFields::publish_deck_angles` implements those calculations.
The frame output phase publishes them after reset/physics completion from
`animated_skeleton.record.pose[0][2]` and the current packet's `board_flipped`.
The next animation phase supplies that retained output to MotionHost; the
behavior copies it on Begin only. No extra per-frame attribute writer or
missing-output fallback was added.

Saved native evidence: research root
`physics-comparison-2026-09-02/evidence/deck-pitch-yaw-20260908`;
the existing Fill decompilation is in `air-motion-graph-2026-09-05` and its
decoded instructions in `riding-outputs-2026-09-04/skate3.decoded-82be1ae8.txt`.
Read-only extraction verified the saved database hashes were unchanged.

Focused regressions cover native quadrants, zero denominators, flipped stance,
raw pitch projection, stock graph binding, missing-output errors and
Begin/Update/End/reentry. These do not replay the user's original input sequence
or establish rendered gameplay parity.

Validation (2026-09-08):

- `cargo test -p skate-core deck_angles --locked`: 1 passed.
- `cargo test -p skate-game --bin skate3rust stock_deck_angles --locked -- --ignored`:
  1 passed against `SKATE3_ASSET_ROOT` pointing to the clone's `assets` directory.
- The existing
  `raw_repeated_walk_turn_throw_recall_jump_remount_preserves_full_roots_and_joints`
  test passed: 5,400 uninterrupted ticks, three walking/turning/throw/recall/jump/
  remount cycles, 7.92 seconds, with its original assertions and collision fixture.
  This is not the University crash input replay.
- The first game-test compilation found the missing procedural
  `maps/format-demo.skate` fixture. Regenerated it with `tools/make_skate_demo.py`;
  no stock world or test assertions were replaced.
