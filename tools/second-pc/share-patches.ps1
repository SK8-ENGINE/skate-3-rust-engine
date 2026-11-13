# Main PC: put a "local hails-additions" in the second PC's share: the pushed hails-additions plus the work on this PC
# that is not pushed yet (unpushed commits, uncommitted and new files of each source worktree), as one patch.
# get-patches.ps1 on the second PC applies it to a launcher-only copy and adds it to the launcher as "hails-local".
# For testing between pushes (outside the commit window); once the work is pushed, hails-additions itself has it.
# Only what git tracks travels (.gitignore respected): no build output, game files, logs or local notes.
# Guide: docs/hails-additions/second-pc-testing.md#local-hails-additions-patches
param(
    # The shared folder (the one setup-main-pc.ps1 shared as SkateSessions).
    [Parameter(Mandatory = $true)][string]$Folder,
    # Worktrees whose local work goes in (relative to the repository or absolute).
    [string[]]$Source = @('.local\wt-living'),
    # What to test, shown before the game starts in the second PC's launcher (\n for new lines).
    [string]$Notes = '',
    # What the patch applies to.
    [string]$Base = 'origin/hails-additions'
)
$ErrorActionPreference = 'Stop'
# The main checkout (where .local lives), also when this script runs from one of its worktrees.
$Repo = Split-Path (& git.exe -C $PSScriptRoot rev-parse --path-format=absolute --git-common-dir)

# Windows PowerShell 5.1 turns a native program's stderr into errors, fatal under Stop: run git with Continue.
function Git([string]$Dir) {
    $ErrorActionPreference = 'Continue'
    $out = & git.exe -C $Dir @args 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "git $($args -join ' ') in $Dir failed: $(($out | Out-String).Trim())" }
    return $out
}

# Absolute, so the git calls that run in other folders find the patch files (a relative -Folder failed there).
$Folder = (Resolve-Path $Folder).Path
# `powershell -File` passes `-Source a, b` as one string "a,b": split it.
$Source = @($Source | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
Git $Repo fetch -q origin | Out-Null
$baseSha = (Git $Repo rev-parse "$Base^{commit}" | Select-Object -Last 1).Trim()
$out = Join-Path $Folder '_patches'
New-Item -ItemType Directory -Force $out | Out-Null
$work = Join-Path $out 'work'
New-Item -ItemType Directory -Force $work | Out-Null
Get-ChildItem $work -File | Remove-Item -Force

# A throwaway worktree at the base: every source patch is applied there first, so a conflict shows up here, not on
# the second PC. One folder, reused; removed again at the end.
$stage = Join-Path $Repo '.local\patch-stage'
if (Test-Path $stage) { Git $Repo worktree remove --force $stage | Out-Null }
Git $Repo worktree add -q --detach $stage $baseSha | Out-Null
$sources = @()
try {
    $i = 0
    foreach ($s in $Source) {
        $dir = if ([IO.Path]::IsPathRooted($s)) { $s } else { Join-Path $Repo $s }
        if (-not (Test-Path (Join-Path $dir '.git'))) { throw "$s is not a worktree." }
        $branch = (Git $dir rev-parse --abbrev-ref HEAD | Select-Object -Last 1).Trim()
        $head = (Git $dir rev-parse --short HEAD | Select-Object -Last 1).Trim()
        # From the pushed branch when hails-additions already has it (only the local work travels), else from where
        # the two meet (the unmerged pushed commits travel too).
        $from = ''
        $ErrorActionPreference = 'Continue'
        & git.exe -C $dir rev-parse -q --verify "origin/$branch^{commit}" 2>$null | Out-Null
        $hasUpstream = $LASTEXITCODE -eq 0
        if ($hasUpstream) { & git.exe -C $dir merge-base --is-ancestor "origin/$branch" $baseSha 2>$null; $merged = $LASTEXITCODE -eq 0 } else { $merged = $false }
        $ErrorActionPreference = 'Stop'
        if ($merged) { $from = "origin/$branch" } else { $from = (Git $dir merge-base $baseSha HEAD | Select-Object -Last 1).Trim() }
        $unpushed = (Git $dir rev-list --count "$from..HEAD" | Select-Object -Last 1).Trim()
        # The working tree as git would commit it (new files included), through a temporary index: the worktree's own
        # index and files are not touched.
        $patch = Join-Path $work ("source-{0}.patch" -f $i)
        $index = Join-Path $work ("index-{0}" -f $i)
        $env:GIT_INDEX_FILE = $index
        try {
            Git $dir read-tree HEAD | Out-Null
            Git $dir add -A | Out-Null
            Git $dir diff --cached --binary "--output=$patch" $from | Out-Null
            $changed = @(Git $dir diff --cached --name-only $from).Count
        } finally { Remove-Item Env:\GIT_INDEX_FILE; Remove-Item $index -Force -ErrorAction SilentlyContinue }
        $sources += "source=$s branch=$branch head=$head from=$from unpushed_commits=$unpushed files=$changed"
        if ((Get-Item $patch).Length -gt 0) {
            $ErrorActionPreference = 'Continue'
            $msg = & git.exe -C $stage apply --index --3way --binary $patch 2>&1
            $code = $LASTEXITCODE
            $ErrorActionPreference = 'Stop'
            if ($code -ne 0) { throw "The work in $s does not apply to $Base (merge hails-additions into it first):`n$(($msg | Out-String).Trim())" }
        }
        $i++
    }
    $final = Join-Path $work 'hails-local.patch'
    Git $stage diff --cached --binary "--output=$final" $baseSha | Out-Null
    $files = @(Git $stage diff --cached --name-status $baseSha)
} finally {
    Git $Repo worktree remove --force $stage | Out-Null
}

# Publish: the patch and its description replace the old ones together (patch.txt last, the second PC reads it first).
$hash = (Get-FileHash $final -Algorithm SHA256).Hash
Copy-Item $final (Join-Path $out 'hails-local.patch') -Force
$files | Set-Content -Encoding ASCII (Join-Path $out 'files.txt')
$subject = (Git $Repo log -1 --format=%s $baseSha | Select-Object -Last 1)
# A patch that changes what the game files must be (the menu movie set version, setup's pipeline versions, the menu
# export) can force Refresh Game Files on the first launch: say so in the notes (laptop 2026-10-19: a play session
# turned into a refresh nobody expected). The second PC refreshes after the prebuild when it sees this line.
$versionFiles = 'crates/skate-game/src/retail_menu_movies.rs', 'tools/asset_pipeline/versions.py', 'tools/prepare_menu_movies.py'
$touched = @($files | ForEach-Object { ($_ -split "`t")[-1] } | Where-Object { $versionFiles -contains $_ })
if ($touched.Count -gt 0) {
    $Notes = "FORCES REFRESH GAME FILES: this patch changes asset / menu versions ($($touched -join ', ')); the first launch may restart into Refresh Game Files, so run the refresh after the prebuild.`n" + $Notes
}
$text = @("base=$baseSha", "base_subject=$subject", "created=$((Get-Date).ToString('yyyy-MM-dd HH:mm'))",
    "files=$($files.Count)", "sha256=$hash", "notes=$($Notes -replace "`r?`n", '\n')") + $sources
[IO.File]::WriteAllLines((Join-Path $out 'patch.txt'), [string[]]$text, (New-Object Text.UTF8Encoding $false))
# The second PC's scripts travel with it until they are on hails-additions.
$tools = Join-Path $out 'tools'
New-Item -ItemType Directory -Force $tools | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'get-patches.ps1'), (Join-Path $PSScriptRoot 'GET-PATCHES.bat') $tools -Force
Remove-Item $work -Recurse -Force
Write-Host "Staged in $out : $($files.Count) files on $($baseSha.Substring(0, 8)) ($subject)."
$sources | ForEach-Object { Write-Host "  $_" }
Write-Host 'On the second PC: GET-PATCHES.bat, then pick "hails-local" in the launcher.'
