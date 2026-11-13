---
name: bom-sanitise
description: Find and strip byte order marks (U+FEFF) and other invisible characters (zero-width spaces / joiners, word joiner, soft hyphen) from files, staged changes, commit message files, PR / issue drafts and recent commit history, and write text without them in the first place. Use before every commit, PR edit or publish step whose text came from PowerShell (Set-Content / Out-File add a BOM), when a commit subject or file starts with an invisible character, or whenever the user asks to sanitise BOM characters.
---

# BOM sanitisation

Why: PowerShell 5.1 `Set-Content -Encoding utf8` writes a BOM, and `git commit -F` keeps it in the commit subject.

Tool: `python .claude/skills/bom-sanitise/tools/bom_scrub.py` (run it from the repo or worktree concerned; exit code 1
when something was found and not fixed).

| Check | Command |
|---|---|
| Files or folders (text only; skips `.git`, `target`, `assets`, `data`) | `bom_scrub.py scan PATH...` (add `--fix` to strip) |
| Staged files before a commit | `bom_scrub.py staged` (`--fix` strips and re-stages) |
| A commit message / PR body / issue reply draft | `bom_scrub.py msg FILE` (strips in place, always) |
| The last N commits | `bom_scrub.py log 20` (report only) |
| Text in a pipe | `... \| python bom_scrub.py stdin` |

## Write without a BOM (the cause)
- PowerShell 5.1: `[IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding $false))`. Never
  `Set-Content -Encoding utf8` / `Out-File -Encoding utf8` / `>` for a file another tool reads (they add a BOM;
  `Set-Content` without `-Encoding` writes the ANSI code page instead).
- Prefer the Write tool for message files, PR bodies and docs; Python `open(p, 'w', encoding='utf-8', newline='')`.
- `gh pr edit --body-file`, `git commit -F`, `gh issue comment --body-file`: run `bom_scrub.py msg FILE` first.

## Order before publishing
1. `bom_scrub.py msg <draft>` for commit messages and PR / issue text, `bom_scrub.py staged` for the commit.
2. Then skill `strip-ai-mentions` (AI mentions, em dashes, control characters) where its rules apply.
3. After pushing: `bom_scrub.py log 5` as a check.

## Already pushed
Do not rewrite pushed history (signed commits, merged branches, open PRs) to remove a BOM unless the user
asks; it is invisible on GitHub. Fix the writer instead.

## Files that need a BOM
None in this repo today. If a Windows tool ever demands one, list the file here and do not `--fix` it.
