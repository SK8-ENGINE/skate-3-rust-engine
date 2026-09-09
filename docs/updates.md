# Player updates

Pause → **Updates** opens a native desktop update window. Select **Stable**
(default) or **Latest** (includes experimental prereleases), then **Check now**.
Release notes appear as plain, scrollable text. **Update** authorizes downloading,
verification, a clean game exit, replacement and restart. **Cancel** closes the
offer, or cancels a download before shutdown. No installation occurs on a check.
Save any gameplay you want to retain before choosing Update.

Startup checks run in a separate process, at most once per six hours per channel.
Offline/rate-limit/check failures are silent at startup and visible in manual
checks. Manual checks bypass the six-hour interval. Switching channels clears the
old offer and performs a fresh check; channel changes are disabled during work.
Switching to Stable never downgrades an already installed experimental build;
it waits for a newer stable build. Cancelled offers are checked again on a later
eligible startup, or through the menu.

The helper replaces only `skate3rust.exe`, `support/skate3setup.exe`,
`support/skate3update.exe`, and `release.json`. Assets, maps, settings, mods,
character libraries and setup's installation marker are never replaced. The same
working directory and arguments are used for restart; existing asset selection
continues to work. Each package owns its `data` installation. On the next normal
launch, changed extractor fingerprints trigger an update of affected asset groups;
unchanged groups are retained. Freshly unpacked copies run setup independently.
Explicit `--assets` developer launches bypass this management. Packaged releases are required; developer binaries do not
silently adopt a downloaded build.

Character import and its FBX/native dependencies are embedded in the setup
executable. These are delivered by the original update protocol too, including
to copies that never had a separate Custom Models helper. Importer-only changes
do not require owned-disc extraction. Optional local calibration is preserved.

# Package protocol and release maintenance

`release.json` accompanies the ZIP and is also inside it. Schema 1 identifies the
repository, `windows-x64` target, tag, full source revision, monotonically increasing
release-workflow run number (`build`), and SHA-256 of all three program files.
The executable embeds its revision and build number; its local metadata and hash
must match before checking. Local builds use build 0 and are not update sources.

Build numbers, not lexical tags, timestamps, GitHub's chosen latest release, or
the workspace's currently fixed Cargo version, determine upgrade ordering. Never
reset/replace the release workflow's run-number sequence without a protocol
migration. Rerunning a build preserves its identity and does not offer an upgrade
to an installation of that build. Publish a new release/run to ship another build.
No downgrades are offered across channels. Equal build IDs are not upgrades.

The updater scans pages of 100 published releases, rejects drafts, filters the
channel, and requires the ZIP, checksum and compatible metadata. It selects the
highest build, using release ID only as a deterministic tie breaker. A scan that
exceeds its page/time bound fails rather than claiming the player is up to date.
Older releases without this metadata are skipped. Existing installations predating
the updater need one manual portable-package upgrade to bootstrap it.

The repository and release downloads are public. For private forks/testing,
`SKATE_UPDATE_GITHUB_TOKEN` supplies a token with read access to repository contents.
The updater never stores it and strips authorization on redirects.

## Rolling Experimental

Every push to `main` builds the Windows package. Successful builds update one
mutable prerelease tagged `experimental`; it is never marked as GitHub's Latest
stable release. Manual runs on `main` can publish it too. Builds on other branches
produce only Actions artifacts. Published numbered releases retain the stable
workflow. Do not mark the rolling release immutable.

Select **Latest** in Updates to receive Experimental; **Stable** stays the default
and ignores prereleases. Download the first Experimental ZIP manually if your
installation predates this rolling-release updater.

Builds share this workflow's existing increasing run number. Experimental asset
names include that number: `skate3rust-windows-x64-build-N.zip`, its `.sha256`, and
`release-N.json`. The manifest is uploaded last. The updater ignores incomplete
sets and compares build numbers even when the release tag has not changed. The
current and previous complete sets are kept; a very old pending download can
expire after another publication and should be checked again. The internal ZIP
folder and installed `release.json` names remain unchanged.

Superseded compilation runs are cancelled. Publication is serialized and never
cancelled by a newer push; stale source revisions and older/equal builds are not
published. A failed build leaves the previous download available. Failed uploads
cannot make a partial package eligible. The release body has a current download
link, build/revision identity and ten recent commit summaries. Actions artifacts
expire after seven days; dependency caching reduces repeat compilation cost.

The publisher uses GitHub's workflow token with contents-write permission only in
the publish job. No personal token or game assets are included. Standard hosted
runners are free for public repositories; private forks use their own allowance.

The ZIP checksum is mandatory; GitHub's asset digest is also checked when present.
The internal/external manifests and individual executable hashes must agree.
This trusts the project's GitHub release publisher and HTTPS, not a separate
signing key. Archive paths, duplicate names, links and oversized expansion are
rejected. Only fixed program filenames are written; notes and manifest paths are
never executed. No shell is used. The helper runs from a temporary copy so its
installed binary can also be replaced.

# Failure recovery

Downloads stage under `.update-transaction/new` before requesting a successful
Bevy exit, which the crash supervisor treats as normal. Windows replacement is
retried for up to 45 seconds per file; other running game instances can prevent
replacement. An OS lock serializes updater instances in the same installation.
All four previous files are backed up before the durable journal is published.
Replacement failures restore those backups and restart the old program when
rollback succeeds. Do not close the installer during replacement.

If interrupted, the next normal launch detects the journal and exits to a copied
helper, which restores the old program before restarting. Close other game
instances if recovery cannot replace their locked binaries. Backups remain in
`.update-transaction/old`; keep this directory until recovery succeeds. If the
game executable itself is missing or externally damaged, restore those four backup
files manually or unpack the portable release over the program files. User data
does not need extraction again. Recovery cannot guarantee detection of semantic
bugs in a successfully installed new game; backups are retained for manual return.

# Validation

Automated, isolated checks: `python -m unittest discover -s tools -p test_updater.py -v`.
The release workflow runs these before packaging. Rust integration can be checked
with `cargo check -p skate-game --bin skate3rust --no-default-features`.

Player manual checklist (use a disposable copy of a packaged installation):

- Start an older packaged build: a newer eligible release shows notes and Update/Cancel;
  playing remains possible while checking. Cancel leaves the executable unchanged.
- Open Pause → Updates, check manually, and switch Stable/Latest. Verify prerelease
  filtering, persisted selection after restart, and no stale offer after switching.
- Choose Update, then cancel during download: game stays open and program files
  remain unchanged. Check again and finish: progress, clean exit, one restart,
  correct version, no crash dialog, no ISO/setup prompt, preserved player data.
- Disconnect networking: startup stays playable without a dialog; manual check
  explains the failure. Repeat with a simulated rate-limit response.
- In an isolated fixture, corrupt the ZIP/checksum: failure must precede shutdown.
  Hold a target file locked or deny a replacement: verify rollback and retained
  backups. Interrupt after the journal is written, then launch again: verify recovery.

GitHub references: [release listing, pagination and latest-release semantics](https://docs.github.com/en/rest/releases/releases),
[asset digests](https://docs.github.com/en/rest/releases/assets).
