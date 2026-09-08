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
continues to work. Packaged releases are required; developer binaries do not
silently adopt a downloaded build.

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

At implementation time the configured repository was private with no releases.
Normal players need publicly accessible releases. For private testing, launch with
`SKATE_UPDATE_GITHUB_TOKEN` set to a token with read access to repository contents.
The updater does not store this credential; it uses authenticated API asset URLs
and strips authorization on redirects. Never place tokens in launch arguments,
tracked scripts or release packages. No repository visibility or release was
changed by this implementation.

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
