---
name: conflict-resolution
description: Resolve git merge conflicts between branches and PR branches without losing anything - reuse resolutions already made (an integration branch, an earlier merge), stack conflicting PRs so they merge in order, and prove afterwards that no line from any branch was dropped and the tests still pass. Use whenever a merge, sync or PR combination conflicts, or before claiming branches "merge cleanly together".
---

# Conflict resolution

Goal: branches that are meant to land together merge without conflicts, and no branch's change is lost. Don't
delete any branch until the merged result is verified.

Helper scripts are in `tools/` next to this file (installed under `.claude/skills/conflict-resolution/tools/`).

## 1. Look before merging
- Pairwise conflict map without touching a worktree:
  `git merge-tree --write-tree --name-only A B` (conflicted files are listed after the tree id).
- Find an existing resolution: an integration branch that already merges the same branches. A file is safe to take
  wholesale from it only if no unrelated commit touched it there:
  `git rev-list --no-merges <integration> --not <all source branches> -- <file> | wc -l` must be 0.
- Each worktree owns one branch (`git worktree list`); merge in that branch's worktree, never `git stash`.

## 2. Resolve
In order of preference:
1. **Take the file from a reference that merges exactly the same commits** (`git checkout <ref> -- <file>`).
   Check: after committing, `git diff <ref> HEAD` is empty or only differs by what the reference has extra.
   Trap: an integration branch can carry a LATER branch's lines; a later merge brings them in properly, but the
   intermediate branch must not have them.
2. **Hunk-level reuse:** `python .claude/skills/conflict-resolution/tools/resolve_like.py <ref> <files>` accepts only
   "ours+theirs" / "theirs+ours" with exact context found in the reference; the rest stays as a conflict for you.
   Never loosen it: a fuzzy version once kept one side only and dropped a doc section, an SDK doc block and two
   `validate()` arms.
3. **Pick per hunk:** `python .claude/skills/conflict-resolution/tools/pick_hunks.py <file> <choice>...`, one choice
   per conflict hunk in file order: `o` (ours), `t` (theirs), `ot` / `to` (both, in that order; `ot:SEP` joins them
   with SEP, `\n` escapes allowed). Handles diff3 hunks. Normalise line endings afterwards if the file mixes them.
4. **By hand:** keep both sides' intent. Lists/match arms/`mod` lines: keep both. A line both branches edited (for
   example a macro field list or a system chain tuple): merge the fields of both into one line. Two branches adding a
   line at the same spot (for example a progress label and a timing span in one function) is a classic case: keep
   both lines.
Before committing: `git grep -n -E '^(<<<<<<<|>>>>>>>) '` must be empty; watch for dropped braces.

## 3. Prove nothing was lost (mandatory)
- `python .claude/skills/conflict-resolution/tools/check_lines_kept.py <base> <result> <branch heads...>` with
  `<base>` = the common parent branch of the sources (not `main` when they sit on another PR). Use the OLD heads of
  branches you just moved (record them before merging). Read every hit: only same-line combinations and deliberate
  rewrites are allowed.
- Run it from inside a worktree (outside a repo it sees no files; it exits 2 when it has no files).

## 6. Patterns seen in real merges
- **An auto-merged line can undo a conflict resolution.** Example: one branch moved `write()` in
  `tools/asset_pipeline/map_writer.py` to compress extension blobs on a thread pool, another added an `extensions=`
  parameter with `(tag, schema, payload)` triples. The auto-merged part kept the first branch's fixed
  `extensions=[(b'WMET', ...)]` line, which silently overwrote the new parameter. Fix: build the records the second
  branch's way, then pack them with the first branch's pool carrying the schema; check the output is byte-identical
  to the unthreaded writer. Read the clean hunks next to every conflict, not only the conflict.
- **Id clashes that are not text conflicts.** Settings menus with numbered rows (for example
  `crates/skate-game/src/graphics_menu.rs`): two branches can each add a row with the same row id. Keep both rows,
  give one a free id, and check every place the ids appear (section row lists, the spawn loop, `adjustable`, the
  change handler, the save condition, labels, tests).
- **Numbered docs:** two branches can add a doc with the same number. Check the next free number on every branch
  that will meet before adding one.
- **`mod` lines and registries:** keep both sides' `mod x;` lines and both sides' registry entries.
- **Upstream API changes an older branch must adapt to** (semantic conflicts, the merge itself is clean):
  - #57 (grind query box) replaced `BoardWorldVolume::linear_velocity` with
    `motion: VolumeMotion { linear_velocity, angular_velocity, force_acceleration }`: create volumes with
    `motion: VolumeMotion { linear_velocity: v, ..Default::default() }` and read `.motion.linear_velocity`.
  - #59 (menus) made `apt_scene::Shape::texture` an `Option`:
    `filter_map(|s| s.texture.as_ref().map(|t| t.rgba.clone()))`.
  - #58 (see-through fences) changed the fog family test in `retail_world.wgsl`; keep its families when adding new
    ones.
  - When the target already carries a test that the incoming branch also adds, take one side: duplicate test
    function names do not compile.
- **Semantic conflicts** (merge is clean, build is not: one branch adds a parameter to a function, another adds a
  call with the old arity). When a reference already merges the same branches, list its merge-only fixes: every
  file whose reference history has only source-branch commits must be identical to ours:
  `for f in $(git diff --name-only HEAD <ref>); do [ $(git rev-list --no-merges <ref> --not <old heads> -- $f | wc -l) = 0 ] && echo $f; done`
  then take those files from the reference.
- Every old head is an ancestor: `git merge-base --is-ancestor <old-head> <result>`.
- Build and test the result (skill `build-and-run`; know the pre-existing upstream failures), and when audio
  changed, the e2e bench must stay IDENTICAL (skill `optimisation`).

## 4. PRs that conflict with each other
- Two open PRs touching the same lines cannot both merge cleanly in any order. Either stack them (the later PR
  merges the earlier one, its description says "merges after #N") or combine them into one PR. Maintainers usually
  prefer fewer, themed PRs; ask when unclear.
- After moving a PR branch: update its description so it matches the branch.
- Close PRs or delete branches only after section 3 passes on the pushed result; `gh pr close` without
  `--delete-branch`; delete branches only with the owner's agreement.

## 5. Commits
Merge commits are commits: sign them like any other commit if the project signs, and only make them when asked.
Amend a merge only while it is unpushed; after a push, fix forward.
