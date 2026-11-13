---
name: prior-work-check
description: Before starting a feature, fix or investigation, check whether upstream (SK8-ENGINE/skate-3-rust-engine) or related projects already did it - open/closed PRs, issues, branches, forks, and sibling projects like skate3recomp. Use at the start of any new piece of work, when planning, or when asked "has someone done this already?".
---

# Check for prior work before starting

Why: audio work was once started from scratch while upstream PR #4 "Audio/retail exact player sound" and #1
"audio engine vehicles" already existed, and several "known pre-existing test failures" already had fix PRs
upstream (#17, #18, #19). Ten minutes of searching first avoids duplicate work and gives format knowledge, sample
choices and test fixes for free.

## 1. Upstream PRs and issues (always)
```bash
R=SK8-ENGINE/skate-3-rust-engine
gh pr list -R $R --state all --limit 100 --json number,title,state,author,updatedAt \
  --jq '.[] | "#\(.number) [\(.state)] \(.title) (\(.author.login), \(.updatedAt[:10]))"'
gh issue list -R $R --state all --limit 100 --json number,title,state --jq '.[] | "#\(.number) [\(.state)] \(.title)"'
gh search prs --repo $R "<keywords>"      # e.g. audio sound sfx xma | water | gamepad
gh search issues --repo $R "<keywords>"
```
Read every hit's title; for anything related, read the PR description, the file list
(`gh pr view N -R $R --json files --jq '.files[].path'`) and review comments. Closed/unmerged PRs count too
(rejected approaches, maintainer feedback).

## 2. Your own fork and branches
- `gh pr list -R <your fork> --state all`, local `git branch -a`, your todo / notes folders: something may be
  half-done on another branch.

## 3. Wider search (for features and format/reverse-engineering work)
- Other forks of upstream: `gh api repos/SK8-ENGINE/skate-3-rust-engine/forks --paginate --jq '.[] |
  "\(.full_name) \(.pushed_at)"'`; check recently pushed ones for matching branches
  (`gh api repos/<fork>/branches --jq '.[].name'`).
- Sibling projects: `mchughalex/skate3recomp` (static recompilation; check its licence before reusing anything:
  without a licence it is read/run only, never copy code; see skill `recomp-audio-trace`).
  `gh search repos "skate 3" --limit 30`, `gh search code "<format magic or identifier>"` (for example `SPLC`,
  `ABKC`, `MOIR` for EA audio).
- Related EA projects for any subsystem (render, physics, AI / navigation, streaming, audio): the Need for Speed:
  Most Wanted decompilation (CC0, byte-matching) and the Burnout decompilation work (b5-decomp, BP-Decomp workflow
  with DWARF outlines), plus the debug symbols that shipped with Need for Speed ProStreet (Xbox 360): same-era EA
  engine code with names, useful to recognise shared library functions in Skate 3. Reference only.
- Design reference for multiplayer, mod tooling and a park editor: `Dingo-Shenanigans/ReSkate` (skate. 2025,
  GPL-3.0; ideas only, not code).
- To read retail code, see skills `recomp-research` (the recomp's generated code) and `ghidra` (pseudocode).
- Format tools: vgmstream (EA SNR/SNS/ABK support), Xenia/Xbox modding wikis for EA formats.

## 4. Report before starting
Report briefly: what exists (link, state, author, date), how complete it looks, what can be reused or learned
(licence permitting: upstream code is under the project's own licence; repos without a licence are reference-only),
and whether starting fresh, building on it, or reviewing/testing it is the better move. If a substantial existing
PR covers the work, let the person you work for decide.

## Notes
- Read-only: never comment on, approve, or push to someone else's PR as part of this check.
- Record findings in your plan/todo for the work, including dependencies on and overlaps with other open PRs.
- Credit every project whose findings you use, in docs and PR descriptions.
