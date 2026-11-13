# Second PC: get the main PC's "local hails-additions" from the share (share-patches.ps1 puts it there): the pushed
# hails-additions plus the main PC's work that is not pushed yet, as one patch. It is applied to a launcher-only copy,
# .local\versions\hails-local, and the launcher gets the version "hails-local" (built from that copy on its next launch).
# Run it again whenever the main PC says there is a new patch; it starts from the base again each time.
# Guide: docs/hails-additions/second-pc-testing.md#local-hails-additions-patches
param(
    # Show what the share has and what is applied here, change nothing.
    [switch]$Status,
    # Remove the hails-local copy and its launcher version.
    [switch]$Remove,
    # Read the patch from this folder instead of the main PC's share (a test, or a copy on a USB stick).
    [string]$Share = ''
)
$ErrorActionPreference = 'Stop'
# The checkout this script is part of (tools\second-pc); a copy run from the share's tools folder needs -Root.
$Root = Split-Path (Split-Path $PSScriptRoot)
if (-not (Test-Path (Join-Path $Root 'Cargo.toml'))) { throw "Copy this script into your checkout's tools\second-pc\ first (it found no checkout at $Root)." }
Set-Location $Root
$wt = Join-Path $Root '.local\versions\hails-local'
$stateDir = Join-Path $Root '.local\steam-launcher'
$versionsFile = Join-Path $stateDir 'versions.json'
$configFile = Join-Path $Root 'send-sessions.txt'
$id = 'hails-local'

function Git([string]$Dir) {
    # Windows PowerShell 5.1 turns a native program's stderr into errors, fatal under Stop: run git with Continue.
    $ErrorActionPreference = 'Continue'
    $out = & git.exe -C $Dir @args 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "git $($args -join ' ') failed: $(($out | Out-String).Trim())" }
    return $out
}
function Read-Pairs([string]$File) {
    $c = [ordered]@{}
    foreach ($line in Get-Content $File -Encoding UTF8) { if ($line -match '^\s*(\w+)\s*=\s*(.*?)\s*$' -and -not $c.Contains($Matches[1])) { $c[$Matches[1]] = $Matches[2] } }
    return $c
}
function Get-Share {
    if ($Share) {
        $dir = if (Test-Path (Join-Path $Share 'patch.txt')) { $Share } else { Join-Path $Share '_patches' }
        if (-not (Test-Path (Join-Path $dir 'patch.txt'))) { throw "No patch.txt in $Share." }
        return $dir
    }
    if (-not (Test-Path $configFile)) { throw 'Not set up: run play-and-send.ps1 -Setup first (the same share is used).' }
    $c = Read-Pairs $configFile
    foreach ($server in @($c.main_pc, $c.main_ip) | Where-Object { $_ }) {
        $unc = "\\$server\$($c.share)"
        $ErrorActionPreference = 'Continue'
        & net.exe use $unc /persistent:no 2>&1 | Out-Null
        $ErrorActionPreference = 'Stop'
        # A refused address (wrong account) is not fatal: the next one may answer.
        $found = $false
        try { $found = Test-Path "$unc\_patches\patch.txt" } catch { $found = $false }
        if ($found) { return "$unc\_patches" }
    }
    throw 'The main PC did not answer, or it has not shared a patch yet (share-patches.ps1 on the main PC).'
}
function Load-Versions {
    if (-not (Test-Path $versionsFile)) { return @() }
    return @(Get-Content $versionsFile -Raw | ConvertFrom-Json)
}
function Save-Versions($list) {
    New-Item -ItemType Directory -Force $stateDir | Out-Null
    # An empty pipeline arrives as $null: never write a [null] entry.
    $json = ConvertTo-Json -InputObject @(@($list) | Where-Object { $_ }) -Depth 6
    [IO.File]::WriteAllText($versionsFile, $json, (New-Object Text.UTF8Encoding $false))
}
$appliedFile = Join-Path $wt 'bin\patch.txt'
$launcherDir = 'tools\steam-launcher'
# The tools this PC runs from this checkout (the launcher, these scripts): a patch that changes them updates them here
# too. Each run first puts them back to this checkout's own commit, so the checkout stays clean for a later pull once
# the change is pushed.
$toolDirs = @($launcherDir, 'tools\second-pc')
function Restore-Tools {
    $dirs = @($toolDirs | Where-Object { Test-Path (Join-Path $Root $_) })
    if (-not $dirs) { return $false }
    # Whether an earlier patch had changed them (then the exe was built from those sources and needs a rebuild).
    $changed = [bool](Git $Root status --porcelain -- @dirs | Where-Object { $_ -match '^ ?M' })
    Git $Root checkout -q -- @dirs | Out-Null
    return $changed
}
# The files of $toolDirs the patched copy has different from this checkout, copied over; the count.
function Update-Tools {
    $n = 0
    foreach ($d in $toolDirs) {
        $from = Join-Path $wt $d
        $to = Join-Path $Root $d
        if (-not (Test-Path $from) -or -not (Test-Path $to)) { continue }
        Get-ChildItem $from -File | Where-Object { $_.Extension -in '.cs', '.ps1', '.bat', '.json', '.md' } | ForEach-Object {
            $target = Join-Path $to $_.Name
            if (-not (Test-Path $target) -or (Get-FileHash $_.FullName).Hash -ne (Get-FileHash $target).Hash) {
                Copy-Item $_.FullName $target -Force
                $n++
            }
        }
    }
    return $n
}
# SkateLauncher.exe from this checkout's sources. A running launcher's exe cannot be overwritten, but it can be
# renamed: the new one starts next time.
function Build-Launcher {
    $to = Join-Path $Root $launcherDir
    if (-not (Test-Path (Join-Path $to 'SkateLauncher.cs'))) { return }
    $exe = Join-Path $to 'SkateLauncher.exe'
    $old = Join-Path $to 'SkateLauncher.old.exe'
    if (Test-Path $old) { Remove-Item $old -Force -ErrorAction SilentlyContinue }
    if (Test-Path $exe) { Rename-Item $exe 'SkateLauncher.old.exe' -ErrorAction SilentlyContinue }
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $csc /nologo /target:winexe "/out:$exe" (Join-Path $to 'SkateLauncher.cs') | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $exe)) {
        if (Test-Path $old) { Rename-Item $old 'SkateLauncher.exe' -ErrorAction SilentlyContinue }
        throw 'The launcher did not build: the old launcher stays.'
    }
}

# A copy folder without its worktree (`worktree remove` leaves the untracked `assets` junction behind, laptop
# 2026-10-20): unlink junctions only (rmdir on a link never touches the game files it points to), then the empty
# folder. Anything else in it stops here, so nothing real is deleted.
function Clear-Leftover {
    if (-not (Test-Path $wt) -or (Test-Path (Join-Path $wt '.git'))) { return }
    foreach ($link in @(Get-ChildItem -LiteralPath $wt -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint })) {
        & cmd.exe /c rmdir "$($link.FullName)" | Out-Null
    }
    if (@(Get-ChildItem -LiteralPath $wt -Force).Count -gt 0) {
        throw "$wt holds files but no worktree: move them away, then run this again."
    }
    Remove-Item -LiteralPath $wt -Force
}

if ($Remove) {
    if (Restore-Tools) { Build-Launcher }
    if (Test-Path (Join-Path $wt '.git')) { Git $Root worktree remove --force $wt | Out-Null }
    Clear-Leftover
    Save-Versions (@(Load-Versions) | Where-Object { $_.id -ne $id })
    Write-Host 'Removed the hails-local copy and its launcher version.'
    return
}

$share = Get-Share
$info = Read-Pairs "$share\patch.txt"
Write-Host "Share: patch from $($info.created), $($info.files) files on $($info.base.Substring(0, 8)) ($($info.base_subject))."
if ($info.notes) { Write-Host "  To test: $($info.notes -replace '\\n', ' ')" }
if (Test-Path $appliedFile) { $have = Read-Pairs $appliedFile; Write-Host "Here:  patch from $($have.created) applied." } else { Write-Host 'Here:  none applied yet.' }
if ($Status) { return }

# One game at a time, and never while the launcher plays or builds.
if (Get-Process skate3rust, skate3 -ErrorAction SilentlyContinue) { throw 'A game is running: close it first.' }
if (Test-Path (Join-Path $stateDir 'PLAYING.txt')) { throw 'The launcher is playing (PLAYING.txt): try again after the session.' }

# Copy the patch and check it is the one patch.txt describes (a half-copied share is caught here).
$local = Join-Path $Root '.local\hails-local.patch'
Copy-Item "$share\hails-local.patch" $local -Force
if ((Get-FileHash $local -Algorithm SHA256).Hash -ne $info.sha256) { throw 'The copied patch does not match patch.txt (the main PC may be writing a new one): run it again.' }

# The base commit: the pushed hails-additions the patch was made on.
Git $Root fetch -q origin | Out-Null
$ErrorActionPreference = 'Continue'
& git.exe -C $Root cat-file -e "$($info.base)^{commit}" 2>$null
$known = $LASTEXITCODE -eq 0
$ErrorActionPreference = 'Stop'
if (-not $known) { Git $Root fetch -q https://github.com/Hailey-Ross/rusty-trucks.git hails-additions | Out-Null }

# A launcher-only copy, never worked in: back to the base each time (the last patch's changes and new files go), then
# the new patch, staged, so the launcher's working-tree build sees new files too.
if (-not (Test-Path (Join-Path $wt '.git'))) {
    Clear-Leftover
    New-Item -ItemType Directory -Force (Split-Path $wt) | Out-Null
    Git $Root worktree add -q --detach $wt $info.base | Out-Null
} else {
    Git $wt reset -q --hard $info.base | Out-Null
    Git $wt clean -fdq | Out-Null
}
if ((Get-Item $local).Length -gt 0) {
    Git $wt apply --check --index --binary $local | Out-Null
    Git $wt apply --index --binary $local | Out-Null
}
Remove-Item $local -Force
New-Item -ItemType Directory -Force (Join-Path $wt 'bin') | Out-Null
Copy-Item "$share\patch.txt" $appliedFile -Force
$restored = Restore-Tools
$updated = Update-Tools
if ($restored -or $updated -gt 0) {
    Build-Launcher
    if ($updated -gt 0) { Write-Host "Launcher and tools updated from the patch ($updated file(s)); restart the launcher to use it." }
    else { Write-Host 'Launcher and tools back to this checkout''s own version; restart the launcher to use it.' }
}

# The launcher version: built from this copy as it is (rebuilt whenever a new patch changes it).
$notes = "Local hails-additions from the main PC: hails-additions $($info.base.Substring(0, 8)) plus its unpushed work (patch $($info.created), $($info.files) files)."
if ($info.notes) { $notes += "`n" + ($info.notes -replace '\\n', "`n") }
$entry = [pscustomobject]@{ id = $id; name = 'hails-local (hails-additions + main PC work in progress)'; branch = 'hails-additions'
    dir = '.local/versions/hails-local'; dev = $true; notes = $notes }
$list = @(Load-Versions | Where-Object { $_.id -ne $id }) + $entry
Save-Versions $list
Write-Host "Applied: $($info.files) files. In the launcher pick the version 'hails-local'; its first launch builds it."
