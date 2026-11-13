# Couch / Steam launcher: start any engine version (hails-additions, upstream main, your branches, open PRs), the
# recomp with a tracing option or retail Skate 3 in RPCS3, from the couch with a controller. SkateLauncher.exe (the
# window Steam starts) draws the menu and runs this script for the work; it reads the output below.
# Windows PowerShell 5.1 compatible. See README.md.
#   launcher.ps1 <entry> [label] [-Version <id>] [-NoPrompt]   run an entry (rust-*, recomp-*, rpcs3-*, tool-*)
#   launcher.ps1 entries                                      the entries: id, name, hint, ask, group, on (1/0), why off
#   launcher.ps1 versions                                     the versions: id, name, status, selected
#   launcher.ps1 version-next | version-prev | version-set <id>   select a version (prints its notes)
#   launcher.ps1 notes [id]                                   what to test in a version
#   launcher.ps1 build <id>                                   set up / update / build a version without playing
#   launcher.ps1 pending                                      check a session that closed before its check
#   launcher.ps1 status                                       one line per fact for the window's header (key, value)
#   launcher.ps1 sizes                                        per version: id, name, build cache bytes, bin bytes
#   launcher.ps1 clear-cache <id|all> | clear-build <id|all>  delete build caches / builds (cache + bin\)
#   launcher.ps1 room <id>                                    apply the build size cap as before a build of <id>
#   launcher.ps1 setup-check                                  what the first-time setup finds (see setup.ps1)
# While a game runs, PLAYING.txt exists in the state folder: tools and agents should launch nothing while it does.
param([string]$Entry, [string]$Label, [switch]$NoPrompt, [string]$Version, [string]$Path)

$ErrorActionPreference = 'Stop'
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = (Resolve-Path (Join-Path $Here '..\..')).Path
# Standalone (the launcher-only download, STANDALONE.txt next to this script): no checkout around it. Config and state
# live here, and each version is its own single-branch clone in versions\<id>, made the first time it is played or
# built, so a version you never use is never downloaded or built (user 2026-10-19).
$Standalone = Test-Path (Join-Path $Here 'STANDALONE.txt')
if ($Standalone) { $Repo = $Here }

# Config and state: next to the script when it lives outside tools\ (a private copy), otherwise in
# .local\steam-launcher\ (gitignored), so the repository stays clean.
$StateDir = $Here
if ($Here -like (Join-Path $Repo 'tools') + '\*') { $StateDir = Join-Path $Repo '.local\steam-launcher' }
New-Item -ItemType Directory -Path $StateDir -Force | Out-Null
$Playing = Join-Path $StateDir 'PLAYING.txt'
$LastSession = Join-Path $StateDir 'last_session.txt'
$SelectedFile = Join-Path $StateDir 'version.txt'
$script:BuildLog = ''
$StateLogs = Join-Path $Repo '.local\audio-state-logs'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

function Read-Json([string]$Path) {
    # PowerShell 5.1 returns a JSON array as one object: callers unroll arrays with ForEach-Object.
    return (Get-Content $Path -Raw | ConvertFrom-Json)
}
$Config = $null
$ConfigFile = Join-Path $StateDir 'config.json'
if (Test-Path $ConfigFile) { $Config = Read-Json $ConfigFile }
function Setting([string]$Name, $Default) { if ($Config -and $null -ne $Config.$Name -and "$($Config.$Name)" -ne '') { return $Config.$Name }; return $Default }
$Remote = Setting 'remote' 'origin'
# A configured path, or '' while it is unset or still the config.example.json placeholder.
function Setting-Path([string]$Name, [string]$Sub = '') {
    $o = $Config; if ($Sub) { if (-not $o -or $null -eq $o.$Sub) { return '' }; $o = $o.$Sub }
    if (-not $o -or $null -eq $o.$Name) { return '' }
    $v = [string]$o.$Name; if ($v -like 'C:\path\to*') { return '' }; return $v
}
# First-time setup (the window's wizard): writes the dotted keys of a JSON file into config.json, keeping the rest.
function Set-ConfigValues([string]$File) {
    $pairs = Read-Json $File
    $c = if (Test-Path $ConfigFile) { Read-Json $ConfigFile } else { [pscustomobject]@{} }
    foreach ($p in $pairs.PSObject.Properties) {
        $parts = $p.Name.Split('.'); $o = $c
        for ($i = 0; $i -lt $parts.Count - 1; $i++) {
            if ($null -eq $o.($parts[$i])) { $o | Add-Member -NotePropertyName $parts[$i] -NotePropertyValue ([pscustomobject]@{}) }
            $o = $o.($parts[$i])
        }
        $k = $parts[-1]
        if ($o.PSObject.Properties[$k]) { $o.$k = $p.Value } else { $o | Add-Member -NotePropertyName $k -NotePropertyValue $p.Value }
    }
    [IO.File]::WriteAllText($ConfigFile, ($c | ConvertTo-Json -Depth 20) + "`n", (New-Object Text.UTF8Encoding $false))
}

# The player's Skate 3 DLC packages (Tools > Import DLC copies them here): every game, setup and refresh the launcher
# starts gets the folder as SKATE3_DLC_DIR, so setup converts what they hold (the Create pack's replay editor, DLC
# maps). config.json "dlc_dir" moves it.
$DlcDir = [string](Setting 'dlc_dir' (Join-Path $StateDir 'dlc'))
if (Test-Path -LiteralPath $DlcDir) { $env:SKATE3_DLC_DIR = $DlcDir }

# A Skate 3 DLC package (Xbox 360 LIVE / PIRS / CON, marketplace content 0x2, title 0x454108E6), by its header; returns
# its display name, or $null for anything else.
function Dlc-Name([string]$Path) {
    try {
        $f = [System.IO.File]::OpenRead($Path)
        try { $h = New-Object byte[] 0x492; $n = $f.Read($h, 0, $h.Length) } finally { $f.Dispose() }
    } catch { return $null }
    if ($n -lt $h.Length) { return $null }
    $magic = [System.Text.Encoding]::ASCII.GetString($h, 0, 4)
    if ($magic -notin @('LIVE', 'PIRS', 'CON ')) { return $null }
    $be = { param($o) ([uint32]$h[$o] -shl 24) -bor ([uint32]$h[$o + 1] -shl 16) -bor ([uint32]$h[$o + 2] -shl 8) -bor [uint32]$h[$o + 3] }
    if ((& $be 0x344) -ne 2 -or (& $be 0x360) -ne 0x454108E6) { return $null }
    $name = [System.Text.Encoding]::BigEndianUnicode.GetString($h, 0x411, 0x80).Split([char]0)[0].Trim()
    if (-not $name) { $name = [System.IO.Path]::GetFileName($Path) }
    return $name
}

# Tools > Import DLC: pick package files (one or many) or a folder (every package inside, any depth); each Skate 3
# package is copied to <dlc>\<display name>\<file>, other files (a title update, for example) are skipped. `$From` (the
# launcher's -Path: a package file or a folder) imports without the dialogs, for scripts and the second PC. Returns the
# result line; $script:DlcAdded counts the packages copied.
function Import-Dlc([string]$From = '') {
    $script:DlcAdded = 0
    $packages = { param($folder) @(Get-ChildItem -LiteralPath $folder -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Length -ge 0xB000 } | ForEach-Object { $_.FullName }) }
    if ($From) {
        if (-not (Test-Path -LiteralPath $From)) { return "Import DLC: $From not found." }
        $files = if (Test-Path -LiteralPath $From -PathType Leaf) { @($From) } else { & $packages $From }
        return (Copy-Dlc $files)
    }
    Add-Type -AssemblyName System.Windows.Forms
    $nl = [Environment]::NewLine
    # The tool runs in a windowless PowerShell under the launcher window: a topmost owner keeps the dialogs in front.
    $owner = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true; ShowInTaskbar = $false; Width = 1; Height = 1 }
    $owner.Show(); $owner.Hide()
    $ask = [System.Windows.Forms.MessageBox]::Show($owner, "Pick DLC package files?$nl$nl" + "Yes: choose one or more package files.$nl" +
        'No: choose a folder (every package inside it).', 'Import DLC', [System.Windows.Forms.MessageBoxButtons]::YesNoCancel,
        [System.Windows.Forms.MessageBoxIcon]::Question)
    $files = @()
    if ($ask -eq [System.Windows.Forms.DialogResult]::Yes) {
        $d = New-Object System.Windows.Forms.OpenFileDialog
        $d.Title = 'Skate 3 DLC packages'; $d.Multiselect = $true; $d.Filter = 'All files (*.*)|*.*'
        if ($d.ShowDialog($owner) -ne [System.Windows.Forms.DialogResult]::OK) { return 'Import DLC: cancelled.' }
        $files = @($d.FileNames)
    } elseif ($ask -eq [System.Windows.Forms.DialogResult]::No) {
        $d = New-Object System.Windows.Forms.FolderBrowserDialog
        $d.Description = 'The folder with your Skate 3 DLC packages (subfolders too)'
        if ($d.ShowDialog($owner) -ne [System.Windows.Forms.DialogResult]::OK) { return 'Import DLC: cancelled.' }
        $files = & $packages $d.SelectedPath
    } else { return 'Import DLC: cancelled.' }
    return (Copy-Dlc $files)
}

function Copy-Dlc([string[]]$files) {
    $added = @(); $kept = @(); $skipped = 0
    foreach ($file in $files) {
        $name = Dlc-Name $file
        if (-not $name) { $skipped++; continue }
        $safe = ($name -replace '[\\/:*?"<>|]', '_').Trim('. ')
        $target = Join-Path (Join-Path $DlcDir $safe) ([System.IO.Path]::GetFileName($file))
        if ((Test-Path -LiteralPath $target) -and (Get-Item -LiteralPath $target).Length -eq (Get-Item -LiteralPath $file).Length) { $kept += $name; continue }
        Write-Host "  Copying $name..."
        New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
        Copy-Item -LiteralPath $file -Destination $target -Force
        $added += $name
    }
    if ($added.Count -gt 0 -or $kept.Count -gt 0) { $env:SKATE3_DLC_DIR = $DlcDir }
    $r = "Import DLC: $($added.Count) added" + $(if ($added) { " ($($added -join ', '))" } else { '' }) +
        $(if ($kept) { ", $($kept.Count) already there" } else { '' }) + $(if ($skipped) { ", $skipped other files skipped (not Skate 3 DLC packages)" } else { '' }) + '.'
    $script:DlcAdded = $added.Count
    return $r
}

# Always there: the fork's hails-additions and upstream main, each a launcher-only detached worktree that follows its
# pushed branch and rebuilds when it moved. The URLs can be changed in config.json (fork_url, upstream_url); a
# versions.json entry with the same id replaces the built-in one.
$Builtin = @(
    [pscustomobject]@{ id = 'hails-additions'; name = 'hails-additions (newest on the fork)'; branch = 'hails-additions'
                       url = (Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git'); dir = $(if ($Standalone) { 'versions/hails-additions' } else { '.local/versions/hails-additions' }); detached = $true; release = 'hails-additions'
                       notes = 'Always the newest pushed hails-additions: every launch fetches it and rebuilds when it moved. What changed: docs\hails-additions\RELEASE-NOTES.md.' },
    [pscustomobject]@{ id = 'upstream-main'; name = 'Upstream main (for comparing)'; branch = 'main'
                       url = (Setting 'upstream_url' 'https://github.com/SK8-ENGINE/skate-3-rust-engine.git'); dir = $(if ($Standalone) { 'versions/upstream-main' } else { '.local/versions/upstream-main' }); detached = $true
                       notes = 'Upstream main as it is (SK8-ENGINE), rebuilt when it moved. Use it to compare against hails-additions.' }
)
# config.json "builtin_versions": ["hails-additions"] keeps only the built-ins you use (default: both).
$keepBuiltin = @(Setting 'builtin_versions' @('hails-additions', 'upstream-main') | ForEach-Object { $_ })
$Builtin = @($Builtin | Where-Object { $keepBuiltin -contains $_.id })
$Versions = @()
$VersionsFile = Join-Path $StateDir 'versions.json'
if (Test-Path $VersionsFile) { $Versions = @((Read-Json $VersionsFile) | ForEach-Object { $_ }) }
elseif (-not $Standalone) {
    $Versions = @([pscustomobject]@{ id = 'this'; name = 'This checkout'; branch = ''; dir = '.'; dev = $true
                                     notes = 'This checkout as it is. Add a versions.json to compare more branches (see README.md).' })
}
$own = @($Versions | ForEach-Object { $_.id })
$Versions = @($Builtin | Where-Object { $own -notcontains $_.id }) + $Versions

function Get-Version([string]$Id) {
    if (-not $Id) {
        $Id = $Versions[0].id
        if (Test-Path $SelectedFile) { $s = (Get-Content $SelectedFile -Raw).Trim(); if ($s) { $Id = $s } }
    }
    $v = $Versions | Where-Object { $_.id -eq $Id } | Select-Object -First 1
    # A selected version that left versions.json falls back to the first one.
    if (-not $v -and -not $PSBoundParameters.ContainsKey('Id')) { $v = $Versions[0] }
    if (-not $v) { $v = $Versions[0] }
    return $v
}
function Version-Dir($v) {
    if ($v.dir -eq '.') { return $Repo }
    # The standalone download's versions go to the game folder picked in the first-time setup (config.json "versions_root").
    $vr = Setting-Path 'versions_root'
    if ($vr -and $Standalone -and $v.dir -like 'versions/*') { return (Join-Path $vr ($v.dir.Substring(9))) }
    return (Join-Path $Repo $v.dir)
}
function Invoke-Git([string]$Dir) { $ErrorActionPreference = 'Continue'; & git.exe -C $Dir @args 2>$null }
# Where a version's branch is fetched from and the ref it follows: its own URL (the built-ins) or the remote.
function Version-Source($v) {
    if ($v.url) { return @{ Fetch = $v.url; Ref = "refs/remotes/launcher-$($v.id)/$($v.branch)" } }
    return @{ Fetch = $Remote; Ref = "refs/remotes/$Remote/$($v.branch)" }
}
function Fetch-Version($v, [string]$Dir) {
    $s = Version-Source $v
    Write-Host "  Checking $($v.branch) on $($s.Fetch)..."
    if ($env:SKATE_LAUNCHER_UI -eq '2') { Write-Host "LAUNCHER_WAIT`tbegin`tfetch`t$($v.branch)`t$($s.Fetch)" }
    # Full refspec: a release tag with the branch's name (the fork's rolling build) would win otherwise. No prompt and
    # a stall limit, so a credential helper or a dead network fails the fetch instead of hanging the launcher.
    $keep = $env:GIT_TERMINAL_PROMPT; $env:GIT_TERMINAL_PROMPT = '0'
    & git -C $Dir -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=30 fetch -q $s.Fetch "+refs/heads/$($v.branch):$($s.Ref)" 2>$null | Out-Host
    $fetched = $LASTEXITCODE
    if ($env:SKATE_LAUNCHER_UI -eq '2') { Write-Host "LAUNCHER_WAIT`tend`tfetch`t$fetched" }
    if ($fetched -ne 0) { Write-Host "  Could not reach $($s.Fetch) (git fetch failed); using the last fetched $($v.branch)." }
    $env:GIT_TERMINAL_PROMPT = $keep
    return $s.Ref
}

# One line of status for a version: built (commit) / needs a build / not set up.
# Prebuilt builds (USER-ASKS 463): a version with "release" (a release tag of its repository; the built-in
# hails-additions has one) is downloaded from that release instead of cloned and built while config.json "use_releases"
# is on (default: on for the standalone download). Its folder holds the extracted player package.
function Use-Release($v) { return ([bool]$v.release -and [bool](Setting 'use_releases' $Standalone)) }
function Release-Base($v) {
    $u = if ($v.url) { $v.url } else { Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git' }
    $repo = ($u -replace '\.git$', '') -replace '^https://github\.com/', ''
    return "https://github.com/$repo/releases/download/$($v.release)"
}
function Get-Url([string]$Url, [string]$Out) {
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Out -TimeoutSec 900
}
function Release-Installed($v) {
    $f = Join-Path (Version-Dir $v) 'release.json'
    if (Test-Path $f) { try { return (Read-Json $f) } catch {} }
    return $null
}
# The release folder's PLAY.bat: the game with its session log in logs\ (what PLAY-WITH-LOG.bat does, without the pause).
function Write-ReleasePlay([string]$Dir) {
    $bat = @('@echo off', 'cd /d "%~dp0"', 'if not exist "logs" mkdir "logs"',
        'for /f %%t in (''powershell -NoProfile -Command "Get-Date -Format yyyyMMdd-HHmmss"'') do set "STAMP=%%t"',
        '"%~dp0skate3rust.exe" > "logs\game-%STAMP%.log" 2> "logs\game-%STAMP%.stderr.log"') -join "`r`n"
    [IO.File]::WriteAllText((Join-Path $Dir 'PLAY.bat'), $bat + "`r`n", (New-Object Text.ASCIIEncoding))
}
# Downloads the version's release when it moved (checksum checked) and installs it over the old build; the converted
# game files (data\), logs and settings stay. True = a build is there to play.
function Ensure-Release($v) {
    $ErrorActionPreference = 'Continue'
    $d = Version-Dir $v; $base = Release-Base $v
    $have = Test-Path (Join-Path $d 'skate3rust.exe')
    $tmp = Join-Path $env:TEMP ('skate-release-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    try {
        Write-Host "  Checking the $($v.release) release..."
        try { Get-Url "$base/release.json" (Join-Path $tmp 'release.json') } catch {
            if ($have) { Write-Host '  Could not reach the release; playing the installed build.'; return $true }
            Write-Host "  Could not download $($v.name) ($($_.Exception.Message))."; return $false
        }
        $meta = Read-Json (Join-Path $tmp 'release.json'); $cur = Release-Installed $v
        if ($have -and $cur -and $cur.revision -eq $meta.revision) { Write-ReleasePlay $d; return $true }
        Write-Host "  Downloading $($v.name) build $($meta.build)..."
        $zip = Join-Path $tmp 'package.zip'
        try {
            Get-Url "$base/skate3rust-windows-x64.zip" $zip
            Get-Url "$base/skate3rust-windows-x64.zip.sha256" "$zip.sha256"
        } catch {
            Write-Host "  The download failed ($($_.Exception.Message))."
            return $have
        }
        $want = ((Get-Content "$zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
        if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $want) { Write-Host '  The download does not match its checksum; not installed.'; return $have }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $x = Join-Path $tmp 'x'
        [IO.Compression.ZipFile]::ExtractToDirectory($zip, $x)
        $src = Join-Path $x 'skate3rust-windows-x64'; if (-not (Test-Path $src)) { $src = $x }
        New-Item -ItemType Directory -Path $d -Force | Out-Null
        # release.json last, so a copy that fails part way is downloaded again next time.
        & robocopy.exe $src $d /E /XF release.json /NFL /NDL /NJH /NJS /NP | Out-Null
        if ($LASTEXITCODE -ge 8) { Write-Host '  Could not copy the new build into place (is the game still running?).'; return $have }
        Copy-Item -LiteralPath (Join-Path $src 'release.json') -Destination $d -Force
        Write-ReleasePlay $d
        Write-Host "  Installed $($v.name) build $($meta.build)."
        return $true
    } finally { Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue }
}
function Version-Status($v) {
    if (Use-Release $v) {
        $cur = Release-Installed $v
        if (-not $cur -or -not (Test-Path (Join-Path (Version-Dir $v) 'skate3rust.exe'))) { return 'not downloaded yet (first launch downloads it)' }
        return "release build $($cur.build) ($("$($cur.revision)".Substring(0, 7)))"
    }
    $d = Version-Dir $v
    if (-not (Test-Path (Join-Path $d '.git'))) { return 'not set up yet (first launch builds it)' }
    $exe = Join-Path $d 'bin\skate3rust.exe'
    if (-not (Test-Path $exe)) { return 'not built yet (first launch builds it)' }
    if ($v.dev) { return "built from the working tree ($((Get-Item $exe).LastWriteTime.ToString('MM-dd HH:mm')))" }
    $marker = Join-Path $d 'bin\built_commit.txt'
    $head = (Invoke-Git $d rev-parse --short HEAD)
    $built = ''; if (Test-Path $marker) { $built = (Get-Content $marker -Raw).Trim() }
    if ($built -eq $head) { return "built at $head" }
    return "out of date (built $built, now $head): next launch rebuilds"
}

# The published installation's assets (setup's marker), else the checkout's own assets link.
function Current-Assets {
    # Standalone: the first version that has converted game files shares them with the others (one conversion).
    $bases = @(Join-Path $Repo 'data')
    if ($Standalone) { $bases += @($Versions | ForEach-Object { Join-Path (Version-Dir $_) 'data' }) }
    foreach ($b in $bases) {
        try {
            $marker = Get-Content -Raw (Join-Path $b 'installation.json') | ConvertFrom-Json
            $a = Join-Path $b (($marker.directory -replace '/', '\') + '\assets')
            if (Test-Path $a) { return $a }
        } catch {}
    }
    try {
        $marker = Get-Content -Raw (Join-Path $Repo 'data\installation.json') | ConvertFrom-Json
        $a = Join-Path (Join-Path $Repo 'data') (($marker.directory -replace '/', '\') + '\assets')
        if (Test-Path $a) { return $a }
    } catch {}
    $a = (Get-Item (Join-Path $Repo 'assets') -Force -ErrorAction SilentlyContinue).Target | Select-Object -First 1
    if ($a) { return $a }
    return (Join-Path $Repo 'assets')
}

# Each version builds into its own target folder (cargo would mix the branches' crates in a shared one): the version's
# "target", else <target_root>\<id> from config.json, else the worktree's own target\.
function Target-Dir($v) {
    if ($v.target) { return (Resolve-Local $v.target) }
    $root = Setting 'target_root' ''
    if ($root) { return (Join-Path $root $v.id) }
    return (Join-Path (Version-Dir $v) 'target')
}
function Folder-Bytes([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return [long]0 }
    $sum = [long]0
    try { foreach ($f in [System.IO.Directory]::EnumerateFiles($Path, '*', 'AllDirectories')) { try { $sum += ([System.IO.FileInfo]$f).Length } catch {} } } catch {}
    return $sum
}
# Size cap (config.json "build_cap_gb", e.g. 200): the build folders together, or everything under "build_cap_dir" when
# set (a drive folder other builds share too), stay under the cap. Before a build, while the total plus the room a
# build needs ("build_reserve_gb", default 30) is over it, the launcher deletes the build cache of the version that
# was built longest ago, never the one it is about to build. Only the launcher's own version target folders are
# deleted; a version's bin\ keeps its exe, so it still plays and only its next build starts from scratch.
function Make-Room($v) {
    $cap = [double](Setting 'build_cap_gb' 0)
    if ($cap -le 0) { return }
    $reserve = [double](Setting 'build_reserve_gb' 30)
    $capDir = Setting 'build_cap_dir' ''
    $others = @($Versions | Where-Object { $_.id -ne $v.id } | ForEach-Object { [pscustomobject]@{ V = $_; Dir = (Target-Dir $_) } } |
        Where-Object { (Test-Path -LiteralPath $_.Dir) -and $_.Dir -ne (Target-Dir $v) })
    Write-Host '  Checking the build folders'' size...'
    $total = { if ($capDir) { Folder-Bytes $capDir } else { (@($Versions | ForEach-Object { Target-Dir $_ } | Sort-Object -Unique | ForEach-Object { Folder-Bytes $_ }) | Measure-Object -Sum).Sum } }
    $used = [double](& $total) / 1GB
    Write-Host ("  Build folders: {0:N1} GB of {1:N0} GB (a build needs about {2:N0} GB)." -f $used, $cap, $reserve)
    # Oldest build first: the time of its exe in bin\ (never built = oldest).
    $queue = @($others | Sort-Object { $e = Join-Path (Version-Dir $_.V) 'bin\skate3rust.exe'; if (Test-Path $e) { (Get-Item $e).LastWriteTime } else { [datetime]::MinValue } })
    foreach ($o in $queue) {
        if ($used + $reserve -le $cap) { break }
        $gb = (Folder-Bytes $o.Dir) / 1GB
        Write-Host ("  Over the cap: deleting the build cache of {0} ({1:N1} GB, {2})..." -f $o.V.name, $gb, $o.Dir)
        Remove-Item -LiteralPath $o.Dir -Recurse -Force -ErrorAction SilentlyContinue
        $used = [double](& $total) / 1GB
    }
    if ($used + $reserve -gt $cap) { Write-Host ("  Still {0:N1} GB of {1:N0} GB: nothing more of the launcher's to delete; building anyway." -f $used, $cap) }
}

# Points a checkout's assets junction at the current installation when it no longer resolves: Refresh Game Files
# publishes a new installation and deletes the old one, re-pointing only the link of the copy that ran it, so the
# others failed with "Asset root assets: The system cannot find the path specified" (laptop 2026-10-18). A real
# folder is never touched. Returns a line when it repaired one.
function Repair-AssetLink([string]$Dir, [string]$Name) {
    $link = Join-Path $Dir 'assets'
    $item = Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue
    if ($item -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return '' }
    if ($item -and (Test-Path (Join-Path $link 'private\game.json'))) { return '' }
    $target = Current-Assets
    if (-not (Test-Path (Join-Path $target 'private\game.json'))) { return '' }
    if ($item) { cmd /c rmdir "$link" | Out-Null }
    elseif ($Dir -eq $Repo) { return '' }
    cmd /c mklink /J "$link" "$target" | Out-Null
    return "Assets link of $Name pointed at the current installation."
}

# Every checkout the launcher knows: this one and each version's worktree that exists.
function Repair-AllAssetLinks {
    $lines = @(Repair-AssetLink $Repo 'this checkout')
    foreach ($v in $Versions) {
        $d = Version-Dir $v
        if ($d -ne $Repo -and (Test-Path (Join-Path $d '.git'))) { $lines += Repair-AssetLink $d $v.name }
    }
    return @($lines | Where-Object { $_ })
}

# Self-update (USER-ASKS 116): the launcher brings its own files to the newest pushed hails-additions on the fork, so a
# push reaches the launcher the user runs without git pull or a shell. Its own bare shallow clone in the state folder
# (self.git) holds the fork's branch; the launcher copies tools\steam-launcher (into this folder) and tools\second-pc
# (the scripts it calls) from it. Nothing else of the checkout is touched, nothing is committed, and a local file that
# differs (get-patches.ps1 copies unpushed launcher files in) is backed up to self-update-backup\<time>\ first. Runs
# at every start, before "Update and build", and from Tools; config.json "self_update": false turns the automatic
# runs off, "launcher_branch" picks another branch. Log: self-update.log in the state folder.
$SelfGit = Join-Path $StateDir 'self.git'
$SelfCommit = Join-Path $StateDir 'launcher_commit.txt'
$SelfLog = Join-Path $StateDir 'self-update.log'
$SelfRestart = Join-Path $StateDir 'restart.txt'
$SelfBranch = Setting 'launcher_branch' 'hails-additions'
$SelfUrl = Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git'
$SelfPaths = @('tools/steam-launcher', 'tools/second-pc')
function Self-Log([string]$Text) { Add-Content -Path $SelfLog -Value ((Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + "  $Text") -Encoding UTF8 }
function Self-Short([string]$Sha) { if ($Sha.Length -ge 8) { return $Sha.Substring(0, 8) }; return $Sha }
# The commit the launcher's files came from ('' = never updated by itself: a git checkout's own HEAD then).
function Launcher-Commit {
    if (Test-Path $SelfCommit) { return (Get-Content $SelfCommit -Raw).Trim() }
    if (-not $Standalone) { return ((Invoke-Git $Repo rev-parse HEAD) | Select-Object -First 1) }
    return ''
}
# The fork's branch tip as of the last fetch ('' = never fetched).
function Fork-Commit { if (Test-Path (Join-Path $SelfGit 'HEAD')) { return ((Invoke-Git $SelfGit rev-parse --verify -q refs/heads/self) | Select-Object -First 1) }; return '' }
# Builds SkateLauncher.exe from SkateLauncher.cs. The running exe can be renamed (not overwritten), so the new one takes
# its name and the next start uses it. True = rebuilt.
# The prebuilt exe (USER-ASKS 463): the fork release's SkateLauncher.exe, when its sidecar names this SkateLauncher.cs
# (sha256 of the text without CR) and the download matches the exe hash in it. True = $Out holds it.
function Get-PrebuiltLauncher([string]$Out) {
    $ErrorActionPreference = 'Continue'
    $repo = ((Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git') -replace '\.git$', '') -replace '^https://github\.com/', ''
    $base = "https://github.com/$repo/releases/download/$(Setting 'launcher_branch' 'hails-additions')"
    $side = "$Out.sha256"
    try {
        Get-Url "$base/SkateLauncher.exe.source-sha256" $side
        $f = @{}; foreach ($l in (Get-Content $side)) { $kv = "$l".Trim() -split '\s+'; if ($kv.Count -eq 2) { $f[$kv[0]] = $kv[1].ToLower() } }
        $text = [IO.File]::ReadAllText((Join-Path $Here 'SkateLauncher.cs'), [Text.Encoding]::UTF8).Replace("`r", '')
        $sha = [Security.Cryptography.SHA256]::Create()
        $mine = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($text))) -replace '-', '').ToLower()
        if ($f['source'] -ne $mine) { return $false }
        Get-Url "$base/SkateLauncher.exe" $Out
        if ((Get-FileHash $Out -Algorithm SHA256).Hash.ToLower() -ne $f['exe']) { Remove-Item $Out -ErrorAction SilentlyContinue; return $false }
        return $true
    } catch { Remove-Item $Out -ErrorAction SilentlyContinue; return $false }
    finally { Remove-Item $side -ErrorAction SilentlyContinue }
}
# The switchover (docs/hails-additions/44-every-os.md "Switching launchers"): launcher-flavor.txt says "rust" once
# Tools > Try the new launcher put the Rust launcher in place of SkateLauncher.exe. It keeps itself current from the
# release, so this never puts the classic exe back over it.
function Launcher-IsRust { $f = Join-Path $StateDir 'launcher-flavour.txt'; (Test-Path $f) -and "$(Get-Content $f -Raw)".Trim() -eq 'rust' }
function Switch-ToNewLauncher {
    $ErrorActionPreference = 'Continue'
    $repo = ((Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git') -replace '\.git$', '') -replace '^https://github\.com/', ''
    $base = "https://github.com/$repo/releases/download/$(Setting 'launcher_branch' 'hails-additions')"
    $asset = 'skate-launcher-windows-x64.exe'
    $new = Join-Path $Here 'SkateLauncher.new.exe'; $side = "$new.sha256"
    try {
        Get-Url "$base/$asset.sha256" $side
        $want = ("$(Get-Content $side -Raw)".Trim() -split '\s+')[0].ToLower()
        Get-Url "$base/$asset" $new
        if ((Get-FileHash $new -Algorithm SHA256).Hash.ToLower() -ne $want) { Remove-Item $new -ErrorAction SilentlyContinue; return 'Try the new launcher: the download did not match its checksum; nothing changed.' }
    } catch { Remove-Item $new -ErrorAction SilentlyContinue; return "Try the new launcher: could not download it ($($_.Exception.Message)); nothing changed." }
    finally { Remove-Item $side -ErrorAction SilentlyContinue }
    $exe = Join-Path $Here 'SkateLauncher.exe'; $old = Join-Path $Here 'SkateLauncher.old.exe'; $classic = Join-Path $Here 'SkateLauncher.classic.exe'
    try {
        if (Test-Path $exe) { Copy-Item $exe $classic -Force }
        if (Test-Path $old) { Remove-Item $old -ErrorAction Stop }
        if (Test-Path $exe) { Move-Item $exe $old -Force }
        Move-Item $new $exe -Force
    } catch { Remove-Item $new -ErrorAction SilentlyContinue; return "Try the new launcher: could not swap the launcher ($($_.Exception.Message)); nothing changed." }
    Set-Content -Path (Join-Path $StateDir 'launcher-flavour.txt') -Value 'rust' -Encoding ascii
    Set-Content -Path (Join-Path $StateDir 'restart.txt') -Value 'rust' -Encoding ascii
    'The new launcher is installed and starts now. Tools > Switch back to the classic launcher undoes it.'
}
function Rebuild-LauncherExe {
    if (Launcher-IsRust) { return $true }
    $cs = Join-Path $Here 'SkateLauncher.cs'; $exe = Join-Path $Here 'SkateLauncher.exe'; $old = Join-Path $Here 'SkateLauncher.old.exe'
    $new = Join-Path $Here 'SkateLauncher.new.exe'
    if (-not (Get-PrebuiltLauncher $new)) {
        & (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe') /nologo /target:winexe "/out:$new" $cs | Out-Null
    }
    if (-not (Test-Path $new)) { Remove-Item $new -ErrorAction SilentlyContinue; return $false }
    try {
        if (Test-Path $old) { Remove-Item $old -ErrorAction Stop }
        if (Test-Path $exe) { Move-Item $exe $old -Force }
        Move-Item $new $exe -Force
        return $true
    } catch { Remove-Item $new -ErrorAction SilentlyContinue; return $false }
}
# The start screen's current step (SkateLauncher.exe RenderStart).
function Start-Stage([string]$Text) { if ($env:SKATE_LAUNCHER_UI -eq '2') { Write-Host "LAUNCHER_STAGE`t$Text" } }
# One line for the result. Writes restart.txt when the window's exe was rebuilt (SkateLauncher.exe restarts into it).
function Self-Update([switch]$Force) {
    $ErrorActionPreference = 'Continue'
    Write-Host "  Checking the launcher: $SelfBranch on $SelfUrl..."
    $keep = $env:GIT_TERMINAL_PROMPT; $env:GIT_TERMINAL_PROMPT = '0'
    try {
        if (-not (Test-Path (Join-Path $SelfGit 'HEAD'))) { & git init -q --bare $SelfGit 2>$null | Out-Null }
        # A start check skipped with B (the window kills it during this fetch, b522) can leave a lock behind.
        Get-ChildItem $SelfGit -Recurse -Filter *.lock -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -lt (Get-Date).AddMinutes(-2) } | Remove-Item -ErrorAction SilentlyContinue
        Start-Stage 'Checking GitHub for a newer launcher'
        if ($env:SKATE_LAUNCHER_UI -eq '2') { Write-Host "LAUNCHER_SELF`tfetching" }
        & git -C $SelfGit -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=30 fetch -q --depth 1 $SelfUrl "+refs/heads/$($SelfBranch):refs/heads/self" 2>$null | Out-Null
        $ok = $LASTEXITCODE -eq 0
        if ($env:SKATE_LAUNCHER_UI -eq '2') { Write-Host "LAUNCHER_SELF`tfetched" }
    } finally { $env:GIT_TERMINAL_PROMPT = $keep }
    $sha = Fork-Commit
    if (-not $ok -or -not $sha) { $r = "Launcher update: could not reach $SelfUrl ($SelfBranch); kept the current launcher."; Self-Log $r; return $r }
    if (-not $Force -and (Test-Path $SelfCommit) -and (Get-Content $SelfCommit -Raw).Trim() -eq $sha) { return '' }
    # The fork's files with this checkout's line endings (core.autocrlf=true, as the Windows checkouts have them).
    $tmp = Join-Path $env:TEMP ('skate-launcher-update-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    $zip = "$tmp.zip"
    Start-Stage "Installing the launcher update ($(Self-Short $sha))"
    try {
        & git -C $SelfGit -c core.autocrlf=true archive --format=zip -o $zip refs/heads/self -- @SelfPaths 2>$null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $zip)) { $r = "Launcher update: could not unpack $(Self-Short $sha); kept the current launcher."; Self-Log $r; return $r }
        Expand-Archive -Path $zip -DestinationPath $tmp -Force
        $stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
        $changed = @(); $backedUp = @()
        foreach ($p in $SelfPaths) {
            $src = Join-Path $tmp ($p -replace '/', '\')
            if (-not (Test-Path $src)) { continue }
            $dest = if ($p -eq 'tools/steam-launcher') { $Here } else { Join-Path $Repo ($p -replace '/', '\') }
            foreach ($f in Get-ChildItem $src -Recurse -File) {
                $rel = $f.FullName.Substring($src.Length + 1)
                $target = Join-Path $dest $rel
                $newBytes = [IO.File]::ReadAllBytes($f.FullName)
                if (Test-Path $target) {
                    $oldBytes = [IO.File]::ReadAllBytes($target)
                    # Same text up to line endings counts as the same file.
                    $a = [Text.Encoding]::UTF8.GetString($oldBytes).Replace("`r", ''); $b = [Text.Encoding]::UTF8.GetString($newBytes).Replace("`r", '')
                    if ($a -eq $b) { continue }
                    $bk = Join-Path $StateDir "self-update-backup\$stamp\$p\$rel"
                    New-Item -ItemType Directory -Path (Split-Path $bk) -Force | Out-Null
                    Copy-Item -LiteralPath $target -Destination $bk -Force
                    $backedUp += "$p/$($rel -replace '\\', '/')"
                }
                New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
                [IO.File]::WriteAllBytes($target, $newBytes)
                $changed += "$p/$($rel -replace '\\', '/')"
            }
        }
    } catch {
        $r = "Launcher update: failed while copying ($($_.Exception.Message)); kept what was not copied yet."; Self-Log $r; return $r
    } finally {
        Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
    Set-Content -Path $SelfCommit -Value $sha -Encoding ASCII
    $short = Self-Short $sha
    if ($changed.Count -eq 0) { $r = "Launcher: up to date at $short."; Self-Log $r; return $r }
    Self-Log ("Updated to $short from $SelfUrl ${SelfBranch}: " + ($changed -join ', '))
    if ($backedUp.Count -gt 0) { Self-Log ("Local versions backed up to self-update-backup\${stamp}: " + ($backedUp -join ', ')) }
    $r = "Launcher updated to $short ($($changed.Count) files)."
    if ($backedUp.Count -gt 0) { $r += " Local changes to $($backedUp.Count) of them were backed up (self-update-backup\$stamp)." }
    if ($changed -contains 'tools/steam-launcher/SkateLauncher.cs') {
        if (Rebuild-LauncherExe) { Set-Content -Path $SelfRestart -Value $short -Encoding ASCII; $r += ' The launcher restarts into the new version.'; Self-Log "Rebuilt SkateLauncher.exe at $short." }
        else { $r += ' Could not rebuild SkateLauncher.exe: the old window stays until build.bat runs.'; Self-Log 'SkateLauncher.exe rebuild failed; old exe kept.' }
    }
    return $r
}

# Who else is building into this version's target folder: another launcher (BUILDING-<id>.txt in the state folder,
# written around every launcher build, stale once its pid is gone), else any cargo.exe whose command line names the
# folder (a dev build). Empty when nobody.
function Build-Holder($v, [string]$Target) {
    $mark = Join-Path $StateDir ('BUILDING-' + $v.id + '.txt')
    if (Test-Path $mark) {
        $f = (Get-Content $mark -Raw).Trim() -split "`t"
        if ($f.Count -ge 4 -and $f[0] -ne "$PID" -and (Get-Process -Id ([int]$f[0]) -ErrorAction SilentlyContinue)) {
            return ('{0} is being built by the {1} since {2:HH:mm} (pid {3})' -f $f[1], $f[3], [datetime]$f[2], $f[0])
        }
        Remove-Item $mark -ErrorAction SilentlyContinue
    }
    try {
        $c = Get-CimInstance Win32_Process -Filter "Name='cargo.exe'" -ErrorAction Stop | Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf($Target, [StringComparison]::OrdinalIgnoreCase) -ge 0 } | Select-Object -First 1
        if ($c) { return ('another cargo is using this build folder (pid {0}, started {1:HH:mm})' -f $c.ProcessId, $c.CreationDate) }
    } catch {}
    return ''
}

# Makes sure the version's worktree exists, follows its pushed branch and is built. True = ready.
function Ensure-Version($v) {
    if (Use-Release $v) { return (Ensure-Release $v) }
    $ErrorActionPreference = 'Continue'
    $d = Version-Dir $v
    if (-not (Test-Path (Join-Path $d '.git'))) {
        New-Item -ItemType Directory -Path (Split-Path $d) -Force | Out-Null
        if ($Standalone) {
            # Only this branch, from its own URL (or the fork's); updates fetch it again below.
            $url = if ($v.url) { $v.url } else { Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git' }
            Write-Host "  Setting up $($v.name): downloading $($v.branch) from $url..."
            & git clone -q --single-branch --branch $v.branch $url $d
            if ($LASTEXITCODE -ne 0) { Write-Host '  Could not download it (git clone failed).'; return $false }
        }
    }
    if (-not (Test-Path (Join-Path $d '.git'))) {
        Write-Host "  Setting up $($v.name): new worktree for $($v.branch)..."
        $ref = Fetch-Version $v $Repo
        # A launcher-only copy ("detached": true) follows the pushed branch without checking it out, so it never
        # conflicts with a worktree where that branch is being worked on.
        if ($v.detached -or $v.url) { & git -C $Repo worktree add --detach $d $ref }
        else { & git -C $Repo worktree add $d $v.branch }
        if ($LASTEXITCODE -ne 0) { Write-Host '  Could not create the worktree.'; return $false }
    }
    # Every version shares this checkout's prepared assets (a junction, like the dev setup's own). A setup refresh
    # replaces the installation: a link to the old one no longer resolves, so it is pointed at the current one.
    $fixed = Repair-AssetLink $d $v.name
    if ($fixed) { Write-Host "  $fixed" }
    $dirty = [bool](Invoke-Git $d status --porcelain --untracked-files=no)
    if (-not $v.dev -and $dirty -and $v.branch) {
        # Local edits block the fast-forward: say so instead of silently starting an old build.
        Write-Host "  NOT UPDATED: $($v.name) has local changes, so it stays at its current commit."
    }
    if (-not $v.dev -and -not $dirty -and $v.branch) {
        # Follow the pushed branch (fast-forward only; local work is never thrown away).
        $ref = Fetch-Version $v $d
        $behind = (Invoke-Git $d rev-list --count "HEAD..$ref")
        if ($behind -and [int]$behind -gt 0) { Write-Host "  Updating $($v.branch) ($behind new commits)..."; & git -C $d merge -q --ff-only $ref 2>$null }
    }
    $exe = Join-Path $d 'bin\skate3rust.exe'
    $marker = Join-Path $d 'bin\built_commit.txt'
    $head = (Invoke-Git $d rev-parse --short HEAD)
    if ($v.dev) {
        # A working-tree build: the commit plus a hash of the uncommitted changes, so editing the tree rebuilds it.
        # Hashed as raw bytes by git (diff written to a file), so the console code page cannot change it.
        # Only the exe's inputs (cargo + build.rs, docs/images for the icon, maps for include_bytes): a tools\ edit (setup,
        # Python) is read from the checkout at run time and no longer rebuilds (2026-10-23).
        $inputs = @('--', 'crates', 'vendor', 'Cargo.toml', 'Cargo.lock', '.cargo', 'docs/images', 'maps')
        $null = Invoke-Git $d diff --quiet HEAD @inputs
        if ($LASTEXITCODE -eq 1) {
            $tmp = [IO.Path]::GetTempFileName()
            $null = Invoke-Git $d diff --binary "--output=$tmp" HEAD @inputs
            $hash = "$(Invoke-Git $d hash-object $tmp)".Trim()
            Remove-Item $tmp -ErrorAction SilentlyContinue
            if ($hash.Length -ge 12) { $head = "$head+$($hash.Substring(0, 12))" }
        }
    }
    $built = ''; if (Test-Path $marker) { $built = (Get-Content $marker -Raw).Trim() }
    if ((Test-Path $exe) -and $built -eq $head) { return $true }
    Write-Host "  Building $($v.name) at $head. The first build of a version takes several minutes..."
    $target = Target-Dir $v
    Make-Room $v
    # The full build output goes to <version>\logs\launcher-build-<id>-<time>.log (written by SkateLauncher.exe, which
    # draws the progress from it); the newest 10 per version are kept.
    $logDir = Join-Path $d 'logs'
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    $script:BuildLog = Join-Path $logDir ('launcher-build-' + $v.id + '-' + (Get-Date -Format 'yyyyMMdd_HHmmss') + '.log')
    Get-ChildItem $logDir -Filter ('launcher-build-' + $v.id + '-*.log') -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -Skip 9 | Remove-Item -ErrorAction SilentlyContinue
    # The window (SKATE_LAUNCHER_UI=2) gets plain text plus cargo's own "Building [..] n/total" bar; a terminal run keeps
    # cargo's normal output.
    $ui = $env:SKATE_LAUNCHER_UI -eq '2'
    $keep = @{}
    if ($ui) {
        $cargoEnv = @{ CARGO_TERM_COLOR = 'never'; CARGO_TERM_PROGRESS_WHEN = 'always'; CARGO_TERM_PROGRESS_WIDTH = '120'; CARGO_TERM_PROGRESS_TERM_INTEGRATION = 'false' }
        foreach ($k in $cargoEnv.Keys) { $keep[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $cargoEnv[$k]) }
        Write-Host ("LAUNCHER_BUILD`tbegin`t{0}`t{1}`t{2}" -f $v.name, $head, $script:BuildLog)
    }
    # Cargo says only "Blocking waiting for file lock", never who holds it: name the holder up front (USER-ASKS 72).
    $holder = Build-Holder $v $target
    if ($holder) {
        Write-Host "  Waiting: $holder."
        if ($ui) { Write-Host ("LAUNCHER_BUILD`twait`tbuild`t{0}" -f $holder) }
    }
    # The version's own exe running (a Refresh Game Files run, or that version's game) would fail the link: wait for it
    # (USER-ASKS 72, research b521 4c).
    $exeHeld = { @(Get-Process skate3rust -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }).Count -gt 0 }
    if (& $exeHeld) {
        $rm = Join-Path $StateDir 'REFRESHING.txt'
        $what = if ((Test-Path $rm) -and ((Get-Content $rm -Raw).Trim() -eq $v.name)) { "the game files refresh of $($v.name) is using its exe" } else { "$($v.name) is running from its build folder" }
        Write-Host "  Waiting: $what."
        if ($ui) { Write-Host ("LAUNCHER_BUILD`twait`tbuild`t{0}`thold" -f $what) }
        while (& $exeHeld) { Start-Sleep -Seconds 2 }
        if ($ui) { Write-Host "LAUNCHER_BUILD`twaitend" }
    }
    $mark = Join-Path $StateDir ('BUILDING-' + $v.id + '.txt')
    $who = if ($ui) { 'window' } else { 'shell' }
    Set-Content -Path $mark -Value ("{0}`t{1}`t{2}`t{3}" -f $PID, $v.name, (Get-Date -Format s), $who) -Encoding ASCII
    # A pull that leaves the build inputs unchanged (docs, tools) does not rebuild the game (docs/hails-additions/25-perf-diagnostics.md#build-stamp-for-local-builds).
    $env:SKATE_BUILD_STAMP = 'inputs'
    try { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $d 'scripts\Build.ps1') -TargetDirectory $target; $code = $LASTEXITCODE }
    finally { if (Test-Path $mark) { $own = (Get-Content $mark -Raw) -split "`t"; if ($own[0] -eq "$PID") { Remove-Item $mark -ErrorAction SilentlyContinue } } }
    if ($ui) {
        foreach ($k in $keep.Keys) { [Environment]::SetEnvironmentVariable($k, $keep[$k]) }
        Write-Host ("LAUNCHER_BUILD`tend`t{0}" -f $code)
    }
    if ($code -ne 0 -or -not (Test-Path $exe)) { Write-Host "  Build failed. Build log: $script:BuildLog"; return $false }
    Set-Content -Path $marker -Value $head -Encoding ASCII
    return $true
}

$GameNames = @('skate3', 'skate3rust', 'rpcs3')
function Running-Games { @(Get-Process $GameNames -ErrorAction SilentlyContinue | ForEach-Object { $_.ProcessName + '.exe' } | Sort-Object -Unique) }

# What runs, for messages: a Refresh Game Files run by name (REFRESHING.txt), else the game processes.
function Running-Holder {
    $r = Join-Path $StateDir 'REFRESHING.txt'
    if ((Test-Path $r) -and (Running-Games).Count -gt 0) { return "Refresh game files ($((Get-Content $r -Raw).Trim()))" }
    (Running-Games) -join ', '
}

# A game can outlive the script that started it: Refresh Game Files restarts the game into setup and back (the
# laptop's session check and send ran while setup was still converting, 2026-10-18). Wait until none runs.
function Wait-GameTree {
    $said = $false
    while ((Running-Games).Count -gt 0) {
        if (-not $said) { Write-Host '  The game is still running (it restarted itself, for example to refresh its files)...'; $said = $true }
        Start-Sleep -Seconds 2
    }
}

function Run-Bat([string]$Bat, [string]$Arg) {
    $cmd = '/c ""' + $Bat + '"'
    if ($Arg) { $cmd += ' ' + $Arg }
    $cmd += '"'
    $run = Start-Process -FilePath cmd.exe -ArgumentList $cmd -NoNewWindow -PassThru
    $run.WaitForExit()
    Wait-GameTree
}

# Where engine sessions write logs\: this checkout, or in standalone the selected version's clone.
function Logs-Root { if ($Standalone) { return (Join-Path (Version-Dir (Get-Version $Version)) 'logs') }; return (Join-Path $Repo 'logs') }
function Resolve-Local([string]$Path) { if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }; return (Join-Path $Repo $Path) }
function Recomp-Sessions { $p = '.local\recomp\sessions'; if ($Config.recomp.sessions) { $p = $Config.recomp.sessions }; return (Resolve-Local $p) }

# Starts the recomp with research-hooks tracing (environment variables of the research-hooks branch) and waits.
function Run-Recomp($RE, [string]$Lbl) {
    $r = $Config.recomp
    $rargs = '--skate3_demo_path --skate3_demo_path_signed_in'
    if ($r.args) { $rargs = $r.args }
    $cwd = Split-Path $r.exe
    if ($r.working_dir) { $cwd = $r.working_dir }
    if (-not $RE.trace) { Start-Process -FilePath $r.exe -ArgumentList $rargs -WorkingDirectory $cwd -Wait; return }
    $prefix = $RE.id -replace '^recomp-', ''
    if ($RE.folder) { $prefix = $RE.folder }
    $name = $prefix + '_'; if ($Lbl) { $name += $Lbl + '_' }; $name += (Get-Date -Format 'yyyyMMdd_HHmmss')
    $out = Join-Path (Recomp-Sessions) $name
    New-Item -ItemType Directory -Path $out -Force | Out-Null
    $env:SKATE3_AUDIO_TRACE_FILE = Join-Path $out 'trace.tsv'
    $env:SKATE3_TRACE = $RE.trace
    $env:SKATE3_INPUT_RECORD = Join-Path $out 'pad.txt'
    if ($RE.capture) { $env:SKATE3_AUDIO_CAPTURE = Join-Path $out 'audio.f32' }
    Write-Host "  Session folder: $out"
    Start-Process -FilePath $r.exe -ArgumentList $rargs -WorkingDirectory $cwd -Wait `
        -RedirectStandardOutput (Join-Path $out 'game.out') -RedirectStandardError (Join-Path $out 'game.err')
}

# Starts RPCS3 straight into the game (config.json "rpcs3": exe, game = disc folder or EBOOT.BIN, args) and waits.
function Run-Rpcs3($PE) {
    $r = $Config.rpcs3
    $a = '--no-gui --fullscreen'
    if ($r.args) { $a = $r.args }
    if ($PE.args) { $a = $PE.args }
    Start-Process -FilePath $r.exe -ArgumentList ($a + ' "' + $r.game + '"') -WorkingDirectory (Split-Path $r.exe) -Wait
}

# Checks the session written since $Since; returns a one-line summary.
function Check-Session([string]$Kind, [datetime]$Since, $Ver) {
    if ($Kind -eq 'rust') {
        $root = if ($Ver) { Version-Dir $Ver } else { $Repo }
        $f = Get-ChildItem (Join-Path $root 'logs') -Filter 'game-*.log' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since -and $_.Name -notlike '*.stderr.log' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $f) { return 'No new game log was written.' }
        $err = Join-Path $f.DirectoryName ($f.BaseName + '.stderr.log')
        $panics = 0; if (Test-Path $err) { $panics = @(Select-String -Path $err -Pattern 'panicked' -SimpleMatch).Count }
        $frontend = @(Select-String -Path $f.FullName -Pattern 'AUDIO_NATIVE frontend' -SimpleMatch).Count
        $line = "$($f.Name): " + $(if ($panics -gt 0) { "$panics panic(s) - see $($f.BaseName).stderr.log" } else { 'no panics' })
        if ($frontend -gt 0) { $line += ", $frontend menu / marker sounds logged" }
        return $line
    }
    if ($Kind -eq 'statelog') {
        $f = Get-ChildItem $StateLogs -Filter 'state_*.tsv' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $f) { return 'No new state log was written.' }
        Write-Host '  Checking the state log...'
        $lines = Get-Content $f.FullName
        $cols = ($lines[0] -split "`t").Count
        $bad = @($lines | Select-Object -Skip 1 | Where-Object { ($_ -split "`t").Count -ne $cols }).Count
        $mins = [math]::Round(($lines.Count - 1) / 3600.0, 1)
        $ok = 'OK'; if ($bad -ne 0) { $ok = 'MALFORMED - unusable' }
        return "$($f.Name): $($lines.Count - 1) rows (~$mins min), $bad malformed - $ok"
    }
    if ($Kind -eq 'traceall') {
        $root = if ($Ver) { Version-Dir $Ver } else { $Repo }
        $d = Get-ChildItem (Join-Path $root 'logs') -Directory -Filter 'trace-all-*' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $d) { return 'No new trace-all folder was written.' }
        Write-Host '  Checking the trace-all tables...'
        $parts = @()
        foreach ($name in 'audio_state.tsv', 'frames.tsv') {
            $t = Join-Path $d.FullName $name
            if (-not (Test-Path $t)) { continue }
            $cols = 0; $rows = 0; $bad = 0
            foreach ($l in [IO.File]::ReadLines($t)) {
                # frames.tsv starts with a '# skate3rust frame log v1' comment line before its header.
                if ($l.StartsWith('#')) { continue }
                $n = ($l -split "`t").Count
                if ($cols -eq 0) { $cols = $n; continue }
                $rows++; if ($n -ne $cols) { $bad++ }
            }
            $parts += "$name $rows rows, $bad malformed" + $(if ($bad -ne 0) { ' - MALFORMED - unusable' } else { '' })
        }
        $mb = [math]::Round((Get-ChildItem $d.FullName -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)
        if ($parts.Count -eq 0) { return "$($d.Name): $mb MB, no tables" }
        return "$($d.Name) ($mb MB): " + ($parts -join '; ')
    }
    if ($Kind -eq 'recomp') {
        $d = Get-ChildItem (Recomp-Sessions) -Directory -ErrorAction SilentlyContinue | Where-Object { $_.CreationTime -ge $Since } | Sort-Object CreationTime -Descending | Select-Object -First 1
        if (-not $d) { return 'No new recomp session folder.' }
        $t = Join-Path $d.FullName 'trace.tsv'
        if (-not (Test-Path $t)) { return "$($d.Name): no trace.tsv" }
        $mb = [math]::Round((Get-Item $t).Length / 1MB)
        $check = $null; if ($Config.recomp.trace_check) { $check = Resolve-Local $Config.recomp.trace_check }
        if (-not $check -or -not (Test-Path $check)) { return "$($d.Name): $mb MB trace (no trace_check configured)" }
        Write-Host '  Checking the trace (can take a minute for long sessions)...'
        $out = & py -3.13 $check $t 2>&1 | Select-String 'malformed' | Select-Object -Last 1
        $counts = @([regex]::Matches("$out", ":\s*(\d+)") | ForEach-Object { [int]$_.Groups[1].Value })
        if ($counts.Count -eq 0) { return "$($d.Name): could not read the check result" }
        $bad = ($counts | Measure-Object -Sum).Sum
        $ok = 'OK'; if ($bad -ne 0) { $ok = 'MALFORMED - unusable' }
        return "$($d.Name): $mb MB trace, $bad malformed lines - $ok"
    }
    return ''
}

# Optional: config.json "after_session" names a send script (path relative to the repository, e.g. the second PC's
# play-and-send.ps1), run with -SendOnly after each session.
function Send-Script { if (-not $Config -or -not $Config.after_session) { return $null }; return (Resolve-Local $Config.after_session) }
# Why the send can't run ('' = it can).
function Send-Blocked {
    $script = Send-Script
    if (-not $script) { return 'Not set up: name a send script as "after_session" in config.json (see README.md).' }
    if (-not (Test-Path $script)) { return "Send script not found: $script" }
    # Without its settings the script would ask its setup questions, which nobody answers from Steam.
    # Its settings sit next to it (release ZIP copy) or at the checkout root (tools\second-pc\play-and-send.ps1).
    if (-not ((Test-Path (Join-Path (Split-Path $script) 'send-sessions.txt')) -or (Test-Path (Join-Path $Repo 'send-sessions.txt')))) { return "Not set up: run $(Split-Path $script -Leaf) -Setup once." }
    return ''
}
# Returns one line for the result screen ('' = no send configured).
function Send-Sessions {
    if (-not (Send-Script)) { return '' }
    $blocked = Send-Blocked
    if ($blocked) { return "Send: $blocked" }
    Write-Host '  Sending the logs...'
    try { $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Send-Script) -SendOnly | ForEach-Object { "$_" } | Where-Object { $_ }) }
    catch { return "Send: failed ($($_.Exception.Message))." }
    if ($out.Count -eq 0) { return 'Send: no output.' }
    $sent = @($out | Where-Object { $_ -like 'Sent *' -and $_ -notlike 'Sent * file(s) to GitHub*' }).Count
    if ($sent -gt 0) { return "Send: $sent file(s). $($out[-1])" }
    return "Send: $($out[-1])"
}

# docs/hails-additions/second-pc-testing.md#share-sessions-via-github
$ShareSettings = Join-Path $Repo 'share-sessions.txt'
# Tools > Opt-ins and settings (USER-ASKS 337): the game's own saved settings (settings\*.json beside the converted game
# files), the same files the PC overlay writes, so the two always agree. A field a file lacks is the game's default.
function Game-SettingsDir { Join-Path (Split-Path (Current-Assets) -Parent) 'settings' }
function Read-GameJson([string]$File) {
    $p = Join-Path (Game-SettingsDir) $File
    $o = $null
    if (Test-Path $p) { try { $o = Get-Content -Raw $p | ConvertFrom-Json } catch {} }
    if ($null -eq $o) { $o = New-Object PSObject }
    # The game's default changes (its defaults_version, USER-ASKS 338): a file from before takes the new default
    # once, as the game does when it loads it, so a later "off" sticks.
    if ((Test-Path $p) -and $File -in @('graphics.json', 'audio.json') -and [int](Get-Field $o 'defaults_version') -lt 1) {
        if ($File -eq 'graphics.json') { Set-Field $o 'opt_ins.npc_physics' $true } else { Set-Field $o 'decode_speech' $true }
        Set-Field $o 'defaults_version' 1
    }
    return $o
}
function Write-GameJson([string]$File, $Obj) {
    $dir = Game-SettingsDir
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    # No byte order mark: the game's JSON reader refuses one.
    [IO.File]::WriteAllText((Join-Path $dir $File), ($Obj | ConvertTo-Json -Depth 10), (New-Object Text.UTF8Encoding $false))
}
function Get-Field($Obj, [string]$Path) {
    $o = $Obj
    foreach ($part in $Path.Split('.')) {
        if ($null -eq $o -or -not ($o.PSObject.Properties.Name -contains $part)) { return $null }
        $o = $o.$part
    }
    return $o
}
function Set-Field($Obj, [string]$Path, $Value) {
    $parts = $Path.Split('.'); $o = $Obj
    for ($i = 0; $i -lt $parts.Count - 1; $i++) {
        if (-not ($o.PSObject.Properties.Name -contains $parts[$i])) { $o | Add-Member -NotePropertyName $parts[$i] -NotePropertyValue (New-Object PSObject) }
        $o = $o.($parts[$i])
    }
    if ($o.PSObject.Properties.Name -contains $parts[-1]) { $o.($parts[-1]) = $Value } else { $o | Add-Member -NotePropertyName $parts[-1] -NotePropertyValue $Value }
}
function Same-Value($A, $B) {
    if ($null -eq $A -or $null -eq $B) { return $false }
    if ($A -is [bool] -or $B -is [bool]) { return ([bool]$A -eq [bool]$B) -and ($A -is [bool]) -and ($B -is [bool]) }
    $x = 0.0; $y = 0.0
    if ([double]::TryParse("$A", [ref]$x) -and [double]::TryParse("$B", [ref]$y)) { return [math]::Abs($x - $y) -lt 1e-6 }
    return "$A" -eq "$B"
}
# key, group, label, file, field, values (the first = the game's default), shown values, hint.
$GameRows = @(
    @('npc_draw_distance', 'Opt-ins', 'NPC Draw Distance', 'graphics.json', 'npc_draw_distance', @(1.0, 0.0, 1.5, 2.0, 3.0), @('Retail', 'None (no NPCs, not retail)', '1.5x (not retail)', '2x (not retail)', '3x (not retail)'), 'How far out peds, NPC skaters and cars appear; farther costs frame time.'),
    @('better_water', 'Opt-ins', 'Better Water', 'water-look.json', 'enabled', @($false, $true), @('Retail', 'On (not retail)'), 'A brighter, calmer water look on the canals, fountains and the sea.'),
    @('more_audible', 'Opt-ins', 'More Audible World', 'audio.json', 'more_audible_world', @($false, $true), @('Retail', 'On (not retail)'), 'More cars, peds and NPC skaters heard at once. Applies at the next start.'),
    @('ped_speech', 'Opt-ins', 'Ped Speech', 'audio.json', 'decode_speech', @($true, $false), @('On (default)', 'Off'), 'Peds, the main cast and the announcer talk; setup decodes the speech (about 3.4 GB) at the next Refresh Game Files.'),
    @('extended_replays', 'Opt-ins', 'Extended Replays', 'graphics.json', 'extended_replays', @($false, $true), @('Retail (1:30)', 'On (not retail)'), 'The instant replay keeps more than retail''s 90 seconds (about 85 MB a minute).'),
    @('replay_seconds', 'Opt-ins', 'Replay Length', 'graphics.json', 'replay_seconds', @(120, 150, 180, 210, 240), @('2:00', '2:30', '3:00', '3:30', '4:00'), 'How much the replay keeps with Extended Replays on.'),
    @('better_object_mover', 'Opt-ins', 'Better Object Mover', 'graphics.json', 'better_object_mover', @($false, $true), @('Retail', 'On (not retail)'), 'Every prop can be picked up and moved, not only those with grab handles.'),
    @('debug_menus', 'Opt-ins', 'Debug Menus', 'graphics.json', 'debug_menus', @($false, $true), @('Retail', 'On (not retail)'), 'Shows the debug menus Skate 3 ships hidden; developer leftovers that may not all work.'),
    @('physics_threads', 'Opt-ins', 'Physics Step Threads', 'graphics.json', 'physics_threads_multi', @($false, $true), @('Single (default)', 'Multi'), 'The physics step on one thread (faster on the test PCs) or on Bevy''s multi-threaded scheduler.'),
    @('zombie', 'Opt-ins', 'Zombie Mode', 'graphics.json', 'opt_ins.zombie', @($false, $true), @('Off', 'On'), 'The retail cheat: yellow screen, no traffic, every ped comes after you.'),
    @('npc_physics', 'Opt-ins', 'NPC Skater Physics', 'graphics.json', 'opt_ins.npc_physics', @($true, $false), @('On (default)', 'Off'), 'NPC skaters near you run full skater physics and bail (still being tested).'),
    @('visualize_grabs', 'Opt-ins', 'Visualize Grabs', 'graphics.json', 'opt_ins.visualize_grabs', @($false, $true), @('Off', 'On'), 'Shows the props and car spots in grab range and logs grab availability.'),
    @('vehicle_flee', 'Opt-ins', 'Ped Vehicle Flee', 'graphics.json', 'opt_ins.vehicle_flee', @($false, $true), @('Off', 'On'), 'Peds run from a car within 10 m that drives toward or away from them (still being tested).'),
    @('resolution', 'Settings', 'Resolution', 'graphics.json', 'resolution', @('1280x800', '1280x720', '1600x900', '1920x1080', '1920x1200', '2560x1440', '2560x1600', '3840x2160'), $null, 'The window size.'),
    @('scale', 'Settings', 'Render Scale', 'graphics.json', 'scale', @(100, 25, 50, 67, 75, 85), @('100%', '25%', '50%', '67%', '75%', '85%'), 'The 3D render size against the window; lower is faster.'),
    @('fps', 'Settings', 'Framerate Cap', 'graphics.json', 'fps', @(0, 30, 60, 90, 120, 144, 165, 240), @('Unlimited', '30', '60', '90', '120', '144', '165', '240'), 'The highest frame rate the game runs at.'),
    @('fps_counter', 'Settings', 'FPS Counter', 'graphics.json', 'fps_counter', @('off', 'simple', 'verbose'), @('Off', 'Simple', 'Verbose'), 'The on-screen frame-time readout.'),
    @('master', 'Settings', 'Master Volume', 'audio.json', 'master', @(75, 0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 80, 85, 90, 95, 100), $null, 'Overall game volume.'),
    @('ambience', 'Settings', 'Ambience Volume', 'audio.json', 'ambience', @(100, 0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95), $null, 'The world''s ambience beds.'),
    @('effects', 'Settings', 'Effects Volume', 'audio.json', 'effects', @(100, 0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95), $null, 'Skating and world sound effects.'),
    @('difficulty', 'Settings', 'Difficulty', 'gameplay.json', 'difficulty', @('easy', 'normal', 'hardcore', 'motorized'), @('Easy', 'Normal', 'Hardcore', 'Motorized'), 'The skating difficulty (Custom is set in the game''s Skater menu).')
)
# A row's value as stored: Resolution is width + height, FPS Counter is frame_stats + frame_stats_simple.
function Game-Value($Row, $Obj) {
    switch ($Row[4]) {
        'resolution' { $w = Get-Field $Obj 'width'; $h = Get-Field $Obj 'height'; if ($null -eq $w) { return $null }; return "$($w)x$($h)" }
        'fps_counter' { if (-not (Get-Field $Obj 'frame_stats')) { return $(if ($null -eq (Get-Field $Obj 'frame_stats')) { $null } else { 'off' }) }; if (Get-Field $Obj 'frame_stats_simple') { return 'simple' }; return 'verbose' }
        default { return (Get-Field $Obj $Row[4]) }
    }
}
function Set-GameValue($Row, $Obj, $Value) {
    switch ($Row[4]) {
        'resolution' { $wh = "$Value".Split('x'); Set-Field $Obj 'width' ([int]$wh[0]); Set-Field $Obj 'height' ([int]$wh[1]) }
        'fps_counter' { Set-Field $Obj 'frame_stats' ($Value -ne 'off'); Set-Field $Obj 'frame_stats_simple' ($Value -eq 'simple') }
        default { Set-Field $Obj $Row[4] $Value }
    }
}
function Game-Index($Row, $Obj) {
    $cur = Game-Value $Row $Obj
    if ($null -eq $cur) { return 0 }
    for ($i = 0; $i -lt $Row[5].Count; $i++) { if (Same-Value $Row[5][$i] $cur) { return $i } }
    return -1
}
function Game-Shown($Row, [int]$Index, $Obj) {
    if ($Index -lt 0) { return "$(Game-Value $Row $Obj) (custom)" }
    if ($Row[6]) { return $Row[6][$Index] }
    return "$($Row[5][$Index])"
}
function Fixed-Executor {
    if ((Get-Field (Read-GameJson 'graphics.json') 'physics_threads_multi') -eq $true) { return 'multi' }
    return 'single'
}
function Share-Script {
    foreach ($s in (Join-Path $Here 'share-session-issue.ps1'), (Join-Path $Repo 'tools\second-pc\share-session-issue.ps1')) { if (Test-Path $s) { return $s } }
    return $null
}
function Share-Run([string]$Root, [string[]]$More) {
    $s = Share-Script
    if (-not $s) { return @('share: not available in this launcher') }
    return @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $s -Root $Root -Settings $ShareSettings @More | ForEach-Object { "$_" } | Where-Object { $_ })
}
function Share-Status([string]$Root) {
    if (-not (Share-Script)) { return '' }
    $s = @{}
    if (Test-Path $ShareSettings) { foreach ($l in Get-Content $ShareSettings) { if ($l -match '^\s*([^=#]+?)\s*=\s*(.*)$') { $s[$Matches[1]] = $Matches[2].Trim() } } }
    if ($s['mode'] -ne 'github' -or $s['consent'] -ne '1') { return 'off' }
    $slug = $s['repo']
    if (-not $slug -and $Root) {
        $ErrorActionPreference = 'Continue'
        $url = & git.exe -C $Root remote get-url origin 2>$null | Select-Object -First 1
        $ErrorActionPreference = 'Stop'
        if ($url -match 'github\.com[:/]+([^/]+)/([^/]+?)(\.git)?/?$') { $slug = "$($Matches[1])/$($Matches[2])" }
    }
    if (-not $slug) { return 'no repo' }
    return "on $slug"
}
function Share-Sessions([string]$Root) {
    if ((Share-Status $Root) -notlike 'on *') { return '' }
    Write-Host '  Packing the session for GitHub...'
    $out = Share-Run $Root @()
    $said = @($out | Where-Object { $_ -notlike 'https://*' })
    return "Share: $(if ($said) { $said[-1] } else { 'no output.' })"
}

# The second PC's recomp tools (tools\second-pc\play-recomp.ps1 -Update / -GetSave): config.json "recomp_script", else
# that script once the second PC is set up (send-sessions.txt at the checkout root).
function Recomp-Script {
    $s = Setting 'recomp_script' ''
    if ($s) { return (Resolve-Local $s) }
    $d = Join-Path $Repo 'tools\second-pc\play-recomp.ps1'
    if ((Test-Path $d) -and (Test-Path (Join-Path $Repo 'send-sessions.txt'))) { return $d }
    return $null
}

# Entries. Group: rust (the selected version), recomp, retail (RPCS3), tools. Every entry is always listed; one that
# isn't set up is grayed out in the window with the reason (Why), so the menu shows what could be there.
$Entries = [ordered]@{
    'rust-play'     = @{ Group = 'rust'; Name = 'Play';                       Kind = 'rust';     Hint = "The version's PLAY.bat." }
    'rust-traceall' = @{ Group = 'rust'; Name = 'Play + trace all';           Kind = 'traceall'; Hint = "Every trace (frames, audio, performance, GPU, living world) into logs\trace-all-<time>." }
    'rust-statelog' = @{ Group = 'rust'; Name = 'Play + audio state log';     Kind = 'statelog'; Hint = 'Records the per-frame audio state (.local\audio-state-logs).' }
    'rust-trace'    = @{ Group = 'rust'; Name = 'Play + audio trace';         Kind = 'rust';     Hint = 'Logs every native sound start in logs\game-*.log (SKATE_AUDIO_TRACE=1).' }
    'rust-devmods'  = @{ Group = 'rust'; Name = 'Play with the dev test mods'; Kind = 'rust';    Hint = "Turns on the version's dev test mod (mods_enable in versions.json)." }
    'rust-perf'     = @{ Group = 'rust'; Name = 'Play + performance trace';   Kind = 'rust';     Hint = 'Bevy performance trace (PLAY.bat -trace).' }
    'tool-refresh'  = @{ Group = 'tools'; Name = 'Refresh game files';        Kind = 'tool';     Hint = 'Checks the game files without playing; converts only what changed.' }
    'tool-build'    = @{ Group = 'tools'; Name = 'Update and build the selected version'; Kind = 'tool'; Hint = 'Fetches its branch and builds it now, so the next launch starts at once.' }
    'tool-self-update' = @{ Group = 'tools'; Name = 'Update the launcher';    Kind = 'tool';     Hint = 'Brings the launcher to the newest hails-additions on the fork and rebuilds it (also at every start and before Update and build).' }
    'tool-try-new-launcher' = @{ Group = 'tools'; Name = 'Try the new launcher'; Kind = 'tool'; Hint = 'Puts the new launcher (the same camcorder, rewritten to run on every OS) in place of this one and restarts into it. Your Steam shortcut keeps working; its Tools > Switch back to the classic launcher undoes it.' }
    'tool-send'     = @{ Group = 'tools'; Name = 'Send the logs now';         Kind = 'tool';     Hint = 'Runs the after-session send (config.json after_session).' }
    'tool-share'    = @{ Group = 'tools'; Name = 'Share the last session via GitHub'; Kind = 'tool'; Hint = "Packs the new logs and opens a GitHub issue on the selected version's repository; you drop the zip in. Needs Share sessions turned on." }
    'tool-recomp-update'  = @{ Group = 'tools'; Name = 'Recomp: get the newest build'; Kind = 'tool'; Hint = "Copies the main PC's newest recomp build (play-recomp.ps1 -Update)." }
    'tool-recomp-getsave' = @{ Group = 'tools'; Name = 'Recomp: get the main PC save'; Kind = 'tool'; Hint = "Copies the main PC's recomp save after backing up this PC's (play-recomp.ps1 -GetSave)." }
    'tool-dlc'      = @{ Group = 'tools'; Name = 'Import DLC';                Kind = 'tool';     Hint = 'Copies your Skate 3 DLC packages (pick one or many files, or a folder) to the launcher and converts the new ones (Refresh game files).' }
    'tool-setup'    = @{ Group = 'tools'; Name = 'First-time setup';          Kind = 'tool';     Hint = 'Checks git, Rust, the C++ build tools and Python, and installs what is missing.' }
}
# The session traces a version's source contains; rust-traceall turns on all of them. SKATE_TRACE_ALL (newer
# versions) switches on every diagnostic itself; older versions get the single switches they have.
$SessionTraces = [ordered]@{
    traceall   = 'SKATE_TRACE_ALL'
    statelog   = 'SKATE_AUDIO_STATE_LOG'
    audiotrace = 'SKATE_AUDIO_TRACE'
    frametimes = 'SKATE_FRAME_LOG'
    livingdbg  = 'SKATE_LIVING_WORLD_DEBUG'
}
# The switches a version's source contains (null = not set up yet, so unknown).
function Version-Has($v, [string[]]$Needles) {
    $src = Join-Path (Version-Dir $v) 'crates\skate-game\src'
    if (-not (Test-Path $src)) { return $null }
    $files = Get-ChildItem -Path $src -Filter '*.rs' -Recurse -File | Where-Object { $_.Name -notlike '*tests*' }
    $found = @()
    foreach ($n in $Needles) { if ($files | Select-String -Pattern ('"' + $n + '"') -SimpleMatch -List -Quiet) { $found += $n } }
    return ,$found
}
function Version-Traces($v) {
    $has = Version-Has $v @($SessionTraces.Values)
    if ($null -eq $has) { return ,@() }
    return ,@($SessionTraces.Keys | Where-Object { $has -contains $SessionTraces[$_] })
}
if ($Config -and $Config.recomp_entries) {
    foreach ($re in @($Config.recomp_entries | ForEach-Object { $_ })) {
        $Entries[$re.id] = @{ Group = 'recomp'; Name = $re.name; Kind = $(if ($re.trace -or $re.bat) { 'recomp' } else { 'none' }); Hint = "$($re.hint)"; Recomp = $re }
    }
}
# RPCS3 (PS3 retail) entries from config.json: "rpcs3" { exe, game, args } and "rpcs3_entries" [{ id, name, hint, args }].
if ($Config -and $Config.rpcs3_entries) {
    foreach ($pe in @($Config.rpcs3_entries | ForEach-Object { $_ })) {
        $Entries[$pe.id] = @{ Group = 'retail'; Name = $pe.name; Kind = 'none'; Hint = "$($pe.hint)"; Rpcs3 = $pe }
    }
}
$Labels = @('session', 'ride', 'grind', 'bail', 'push', 'emit', 'carve', 'water', 'marker')

# The play screen's detail panel: what each play option does (Info) and what it records. Records are rows
# "name: what it is"; a row starting with '#' is a section header "TITLE: folder". Every session writes the game log;
# Play + trace all lists what the selected version actually has (trace_all.rs SWITCHES).
$GameLogHead = '#Game log: logs\game-<time>.log'
$GameLogRow = 'game-<time>.log: The game''s own log (errors in .stderr.log)'
$PlayDetails = @{
    'rust-play'     = @{ Info = 'Starts the selected version with no extra traces. For playing and for checking how a change feels.'; Records = @($GameLogHead, $GameLogRow) }
    'rust-traceall' = @{ Info = 'Records every trace the version has in one folder, which the after-session send picks up. The game plays exactly as with Play.' }
    'rust-statelog' = @{ Info = 'Records what the audio saw every frame (speed, wheels, grinds, landings, bails), for tuning sounds against retail.'; Records = @('#Files: .local\audio-state-logs\', 'state_<time>.tsv: Audio state, one row per audio frame', $GameLogHead, 'AUDIO_TIMING: Audio cost each second') }
    'rust-trace'    = @{ Info = 'Writes every native sound start and stop and every gameplay sound event to the game log.'; Records = @($GameLogHead, 'AUDIO_NATIVE: Every native sound start and stop', 'AUDIO_EVENT: Brake, push, grind and landing sound events') }
    'rust-devmods'  = @{ Info = "Turns on the version's dev test mod (mods_enable in versions.json) to try a mod feature before it ships."; Records = @($GameLogHead, $GameLogRow) }
    'rust-perf'     = @{ Info = 'Records a 10 s performance trace of every system and render pass, for finding stutter.'; Records = @('#Files: the version folder', 'trace-downtown-lag.json: Chrome trace, open in Perfetto or chrome://tracing', $GameLogHead, $GameLogRow) }
}
# What Play + trace all records for a version, from the traces its source has.
function TraceAll-Records($v) {
    $t = Version-Traces $v
    $r = @('#Files: logs\trace-all-<time>\')
    if ($t -contains 'traceall' -or $t -contains 'frametimes') { $r += 'frames.tsv: Frame times, one row per frame' }
    if ($t -contains 'traceall' -or $t -contains 'statelog') { $r += 'audio_state.tsv: Audio state, one row per audio frame' }
    if ($t -contains 'traceall') { $r += 'perf.json: Performance report with GPU and render timings' }
    $r += $GameLogHead
    if ($t -contains 'traceall' -or $t -contains 'audiotrace') { $r += 'AUDIO_NATIVE / EVENT: Every sound start, stop and sound event' }
    if ($t -contains 'traceall') { $r += '*_TIMING: Audio, system and fixed-step costs each second' }
    if ($t -contains 'traceall' -or $t -contains 'livingdbg') { $r += 'LIVING_WORLD: Population, NPC skaters and traffic every 5 s' }
    if ($t -contains 'traceall') { $r += 'SKATE_FPS_SAMPLE: Frame rate' }
    if ($t.Count -eq 0) { $r = @('#Not known yet: the version is not set up', 'Its source decides: Set it up once and this lists its traces') }
    return ,$r
}

# What the first-time setup still needs ('' = nothing): setup.ps1's checks (git, Rust, C++ build tools, LLVM, Python,
# Vulkan). The window opens on the setup page while this names something. The selected version is a prebuilt release:
# no build tools (a version built from source asks for them when it is picked).
if (Use-Release (Get-Version $Version)) { $env:SKATE_LAUNCHER_RELEASES = '1' }
. (Join-Path $Here 'setup.ps1')
$script:MissingCache = $null
function Setup-Missing { if ($null -eq $script:MissingCache) { $script:MissingCache = Missing-Tools }; return $script:MissingCache }

# Why an entry can't run now ('' = it can), for the selected version $V.
function Entry-Blocked([string]$Key, $V) {
    $e = $Entries[$Key]
    if ($Key -eq 'tool-setup') { return '' }
    if ($e.Group -eq 'rust' -or $Key -in @('tool-refresh', 'tool-build')) {
        $m = Setup-Missing
        if ($m) { return "Needs $m first: run the first-time setup (Tools)." }
    }
    switch ($Key) {
        'rust-traceall' { if ((Test-Path (Version-Dir $V)) -and (Version-Traces $V).Count -eq 0) { return "$($V.name) has no session traces: use Play." } }
        'rust-statelog' { $h = Version-Has $V @('SKATE_AUDIO_STATE_LOG'); if ($null -ne $h -and $h.Count -eq 0) { return "$($V.name) has no audio state log." } }
        'rust-trace'    { $h = Version-Has $V @('SKATE_AUDIO_TRACE'); if ($null -ne $h -and $h.Count -eq 0) { return "$($V.name) has no audio trace." } }
        'rust-devmods'  { if (-not $V.mods_enable) { return "$($V.name) has no dev test mod (mods_enable in versions.json)." } }
        'tool-refresh'  {
            $h = Version-Has $V @('--update-game-files')
            if ($null -ne $h -and $h.Count -eq 0) { return "$($V.name) can't refresh from here (built before the launcher refresh): use the game's Extras > Refresh Game Files." }
        }
        'tool-send'     { return (Send-Blocked) }
        'tool-share'    { if ((Share-Status (Version-Dir $V)) -notlike 'on *') { return 'Turn on Share sessions via GitHub first (play screen or Tools).' } }
        { $_ -like 'tool-recomp-*' } { if (-not (Recomp-Script)) { return 'Not set up: this PC has no recomp share (tools\second-pc, see docs\hails-additions\second-pc-testing.md), or set recomp_script in config.json.' } }
    }
    if ($e.Recomp) {
        if ($e.Recomp.bat) { if (-not (Test-Path (Resolve-Local $e.Recomp.bat))) { return "Not set up: $($e.Recomp.bat) not found." } }
        elseif (-not $Config.recomp -or -not $Config.recomp.exe -or -not (Test-Path $Config.recomp.exe)) { return 'Not set up: set recomp.exe in config.json (see README.md).' }
    }
    if ($e.Rpcs3) {
        $r = $Config.rpcs3
        if (-not $r -or -not $r.exe -or -not (Test-Path -LiteralPath $r.exe)) { return 'Not set up: set rpcs3.exe in config.json (see README.md).' }
        if (-not $r.game -or -not (Test-Path -LiteralPath $r.game)) { return 'Not set up: set rpcs3.game in config.json (see README.md).' }
    }
    return ''
}

function Save-Result([string]$Text) { Set-Content -Path $LastSession -Value $Text -Encoding UTF8 }

# Tools > Refresh game files for version $Ver: the game's own check without playing (--update-game-files); setup runs
# only for what is out of date (also DLC packs added or removed since the last setup). Returns the result line.
function Refresh-GameFiles($Ver) {
    if (-not (Ensure-Version $Ver)) { return "Refresh game files: $($Ver.name) could not be built. Build log: $script:BuildLog" }
    $d = Version-Dir $Ver
    if (Use-Release $Ver) {
        & (Join-Path $d 'skate3rust.exe') --update-game-files 2>&1 | ForEach-Object { Write-Host "  $_" }
        return "Refresh game files: exit code $LASTEXITCODE."
    }
    . (Join-Path $d 'scripts\Resolve-DevAssets.ps1')
    $assets = Resolve-DevAssets -ProjectRoot $d
    Write-Host "  Checking the game files ($assets)..."
    $said = ''
    # REFRESHING.txt names the refresh to anything that waits for the game to close (USER-ASKS 72).
    $marker = Join-Path $StateDir 'REFRESHING.txt'
    Set-Content -Path $marker -Value $Ver.name -Encoding utf8
    try {
        & (Join-Path $d 'bin\skate3rust.exe') --update-game-files --assets $assets 2>&1 | ForEach-Object { $l = "$_"; if ($l -like 'GAME_FILES *') { $said = $l.Substring(11) }; Write-Host "  $l" }
        $code = $LASTEXITCODE
    } finally { Remove-Item $marker -ErrorAction SilentlyContinue }
    # Setup published a new installation: the other versions' links follow at their next launch.
    switch ($code) { 0 { return "Refresh game files: $said." } 2 { return 'Refresh game files: no game files yet. Convert them first (tools\setup-dev-assets.ps1).' } default { return "Refresh game files: $(if ($said) { $said } else { 'failed' }) (exit code $code)." } }
}

# The tools: no game session, the result goes to last_session.txt like a session's.
function Run-Tool([string]$Key, $Ver) {
    $ErrorActionPreference = 'Continue'
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    switch ($Key) {
        'tool-setup' {
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here 'setup.ps1') -Install
            $r = if ($LASTEXITCODE -eq 0) { 'First-time setup: everything is installed.' } else { 'First-time setup: not finished (see the steps above).' }
        }
        'tool-self-update' { $r = Self-Update -Force; if (-not $r) { $r = 'Launcher: up to date.' } }
        'tool-try-new-launcher' { $r = Switch-ToNewLauncher }
        'tool-build' {
            # The launcher first, so one press brings both to the newest push.
            $self = ''; if ((Setting 'self_update' $true) -ne $false) { $self = Self-Update; if ($self) { Write-Host "  $self" } }
            $r = if (Ensure-Version $Ver) { "Ready: $($Ver.name) ($(Version-Status $Ver))" } else { "Build failed: $($Ver.name). Build log: $script:BuildLog" }
            if ($self -and $self -notlike 'Launcher: up to date*') { $r = "$self
$r" }
        }
        'tool-send' { $r = Send-Sessions }
        'tool-share' { $r = Share-Sessions (Version-Dir $Ver); if (-not $r) { $r = 'Share: off.' } }
        'tool-dlc'  {
            # New packs are converted at once (the laptop import, 2026-10-21: a separate Refresh step was easy to miss).
            $r = Import-Dlc $Path
            if ($script:DlcAdded -gt 0) { Write-Host '  Converting the new DLC (Refresh game files)...'; $r += "`r`n" + (Refresh-GameFiles $Ver) }
        }
        'tool-recomp-update'  { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Recomp-Script) -Update; $r = "Recomp update: exit code $LASTEXITCODE" + $(if ($LASTEXITCODE -eq 0) { ' (done)' } else { ' - see above' }) }
        'tool-recomp-getsave' { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Recomp-Script) -GetSave; $r = "Recomp save: exit code $LASTEXITCODE" + $(if ($LASTEXITCODE -eq 0) { ' (done)' } else { ' - see above' }) }
        'tool-refresh' { $r = Refresh-GameFiles $Ver }
    }
    # A failed build or refresh ends the command line run with exit code 1, so a script stops instead of carrying on
    # with a stale build or export (laptop write-up 2026-11-07 19:18).
    if ($Key -in @('tool-build', 'tool-refresh') -and $r -match '^Build failed|could not be built|\(exit code [1-9]\d*\)\.$|^Refresh game files: exit code [1-9]') {
        $script:ToolFailed = $true
    }
    Save-Result "$stamp  $Key [$($Ver.id)]`r`n$r"
    Write-Host ''
    Write-Host "  $r"
}

function Launch([string]$Key, [string]$Lbl) {
    $e = $Entries[$Key]
    if (-not $e) { Save-Result "Unknown entry: $Key"; return }
    if ((Running-Games).Count -gt 0) { Save-Result "Not started: $(Running-Holder) is running."; return }
    $ver = Get-Version $Version
    $blocked = Entry-Blocked $Key $ver
    if ($blocked) { Save-Result "Not started: $blocked"; Write-Host "  $blocked"; return }
    $verText = ''
    if ($e.Group -eq 'tools') { Run-Tool $Key $ver; return }
    $play = $null
    if ($e.Group -eq 'rust') {
        $verText = " [$($ver.id)]"
        Write-Host ("  Version: " + $ver.name)
        if (-not (Ensure-Version $ver)) {
            Save-Result "Not started: $($ver.name) could not be set up or built.$(if ($script:BuildLog) { " Build log: $script:BuildLog" })"
            return
        }
        $play = Join-Path (Version-Dir $ver) 'PLAY.bat'
        # Without converted game files the game only shows an error; say what to do instead (couch users).
        # The disc picked in the first-time setup: convert the game files from it once, then play.
        $disc = Setting-Path 'disc'
        # A release build's own setup converts them on its first start, from that disc without asking.
        if (Use-Release $ver) { if ($disc) { $env:SKATE3_SETUP_DISC = $disc } }
        elseif ($disc -and (Test-Path -LiteralPath $disc) -and -not (Test-Path (Join-Path (Version-Dir $ver) 'assets\private\stock'))) {
            Write-Host "  Converting the game files from $disc (once; this takes a while)..."
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path (Version-Dir $ver) 'tools\setup-dev-assets.ps1') -Disc $disc
        }
        if (-not (Use-Release $ver) -and -not (Test-Path (Join-Path (Version-Dir $ver) 'assets\private\stock'))) {
            $msg = "Not started: $($ver.name) has no game files (assets\private). Convert them from the disc once: powershell -ExecutionPolicy Bypass -File `"$(Join-Path (Version-Dir $ver) 'tools\setup-dev-assets.ps1')`" -Disc <your Skate 3 ISO or default.xex>."
            Save-Result $msg; Write-Host "  $msg"
            return
        }
        # Per-version environment (versions.json "env": { NAME: value }).
        if ($ver.env) { foreach ($p in $ver.env.PSObject.Properties) { Set-Item -Path ("env:" + $p.Name) -Value ([string]$p.Value) } }
        $env:SKATE_FIXED_EXECUTOR = Fixed-Executor
    }
    $since = Get-Date
    Set-Content -Path $Playing -Value ("entry=$Key label=$Lbl version=$($ver.id) started=" + $since.ToString('yyyy-MM-dd HH:mm:ss')) -Encoding ASCII
    Write-Host ("  Starting: " + $e.Name + $verText)
    try {
        switch ($Key) {
            'rust-play'     { Run-Bat $play '' }
            'rust-statelog' {
                New-Item -ItemType Directory -Path $StateLogs -Force | Out-Null
                $env:SKATE_AUDIO_STATE_LOG = Join-Path $StateLogs ('state_' + (Get-Date -Format 'yyyyMMdd_HHmmss') + '.tsv')
                $env:SKATE_AEMS = '1'; $env:SKATE_AUDIO_TIMING = '1'
                Run-Bat $play ''
            }
            'rust-trace'    { $env:SKATE_AUDIO_TRACE = '1'; Run-Bat $play '' }
            'rust-devmods'  { $env:SKATE3_MODS_ENABLE = $ver.mods_enable; Run-Bat $play '' }
            'rust-perf'     { Run-Bat $play '-trace' }
            'rust-traceall' {
                $traces = Version-Traces $ver
                # Everything in one folder next to the version's game logs (the send script picks it up from there).
                $dir = Join-Path (Version-Dir $ver) ('logs\trace-all-' + (Get-Date -Format 'yyyyMMdd_HHmmss'))
                New-Item -ItemType Directory -Path $dir -Force | Out-Null
                if ($traces -contains 'statelog') {
                    $env:SKATE_AUDIO_STATE_LOG = Join-Path $dir 'audio_state.tsv'
                    $env:SKATE_AEMS = '1'; $env:SKATE_AUDIO_TIMING = '1'
                }
                if ($traces -contains 'audiotrace') { $env:SKATE_AUDIO_TRACE = '1' }
                if ($traces -contains 'frametimes') { $env:SKATE_FRAME_LOG = Join-Path $dir 'frames.tsv' }
                if ($traces -contains 'livingdbg') { $env:SKATE_LIVING_WORLD_DEBUG = '1' }
                if ($traces -contains 'traceall') {
                    # The engine turns on the rest and writes it next to the paths named here.
                    $env:SKATE_TRACE_ALL = '1'
                    $env:SKATE_PERF_REPORT = Join-Path $dir 'perf.json'
                }
                Write-Host ("  Traces: " + ($traces -join ', ') + " -> $dir")
                Run-Bat $play ''
            }
            default {
                if ($e.Rpcs3) { Run-Rpcs3 $e.Rpcs3; Wait-GameTree; break }
                $re = $e.Recomp
                if ($re.label) { $Lbl = $re.label }
                if ($re.bat) { Run-Bat (Resolve-Local $re.bat) $(if ($Lbl -and $Lbl -ne 'session') { $Lbl } else { "$($re.bat_args)" }) }
                else { Run-Recomp $re $(if ($Lbl -eq 'session') { '' } else { $Lbl }); Wait-GameTree }
            }
        }
    } finally {
        Remove-Item $Playing -ErrorAction SilentlyContinue
    }
    Write-Host '  Session ended.'
    $summary = Check-Session $e.Kind $since $ver
    $repaired = @(Repair-AllAssetLinks)
    if ($repaired.Count -gt 0) { $summary = (@($summary) + $repaired) -join "`r`n" }
    # The send script picks up the logs of this version's checkout as well as the main one's
    # (play-and-send.ps1 reads SKATE_SEND_LOGS_DIR; a version's sessions write to its own logs\, laptop 2026-10-18).
    if ($ver) { $env:SKATE_SEND_LOGS_DIR = Version-Dir $ver }
    $sendResult = Send-Sessions
    if ($sendResult) { $summary = (@($summary, $sendResult) | Where-Object { $_ }) -join "`r`n" }
    $shareResult = Share-Sessions $(if ($ver) { Version-Dir $ver } else { $Repo })
    if ($shareResult) { $summary = (@($summary, $shareResult) | Where-Object { $_ }) -join "`r`n" }
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Save-Result ("$stamp  $Key $Lbl$verText`r`n$summary")
}

# Steam's "Exit game" closes the launcher together with the game, so the after-session check can be skipped.
# 'pending' checks the newest session (state log or recomp) if it's newer than last_session.txt and writes the result;
# SkateLauncher.exe runs it at every start. Exit code 10 = a result was written, 0 = nothing pending.
if ($Entry -eq 'pending') {
    # The newest pushed launcher first (not while a game runs: its PLAYING.txt check below still needs the old state).
    $cs = Join-Path $Here 'SkateLauncher.cs'; $exe = Join-Path $Here 'SkateLauncher.exe'; $old = Join-Path $Here 'SkateLauncher.old.exe'
    if (Test-Path $old) { Remove-Item $old -ErrorAction SilentlyContinue }
    $selfSaid = ''
    if ((Setting 'self_update' $true) -ne $false -and $env:SKATE_LAUNCHER_NO_SELF -ne '1' -and (Running-Games).Count -eq 0) { $selfSaid = Self-Update }
    # A pulled or patched launcher updates its own exe too: SkateLauncher.cs newer than SkateLauncher.exe = build it again.
    if (-not (Test-Path $SelfRestart) -and (Test-Path $cs) -and (Test-Path $exe) -and (Get-Item $cs).LastWriteTime -gt (Get-Item $exe).LastWriteTime) {
        Start-Stage 'Building the updated launcher'
        if (Rebuild-LauncherExe) { Set-Content -Path $SelfRestart -Value 'local' -Encoding ASCII }
    }
    # An update that changed files is shown at start like a session result (with the session check's, when one runs).
    if ($selfSaid -like 'Launcher: up to date*') { $selfSaid = '' }
    # A refresh in any copy can leave the others' assets links dangling: repair them at every start.
    if ((Running-Games).Count -eq 0) { Start-Stage 'Checking the game file links'; Repair-AllAssetLinks | ForEach-Object { Write-Host "  $_" } }
    Start-Stage 'Looking for your last session'
    $since = [datetime]::MinValue
    if (Test-Path $LastSession) { $since = (Get-Item $LastSession).LastWriteTime }
    $log = Get-ChildItem $StateLogs -Filter 'state_*.tsv' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $rec = $null; if ($Config) { $rec = Get-ChildItem (Recomp-Sessions) -Directory -ErrorAction SilentlyContinue | Sort-Object CreationTime -Descending | Select-Object -First 1 }
    $kind = ''; $t0 = $since
    if ($log -and $log.LastWriteTime -gt $since) { $kind = 'statelog'; $t0 = $log.LastWriteTime.AddSeconds(-1) }
    if ($rec -and $rec.CreationTime -gt $since -and (-not $log -or $rec.CreationTime -gt $log.LastWriteTime)) { $kind = 'recomp'; $t0 = $rec.CreationTime.AddSeconds(-1) }
    # A trace-all session (rust-traceall, version '.') writes logs\trace-all-<time>\ in the repository.
    $ta = Get-ChildItem (Logs-Root) -Directory -Filter 'trace-all-*' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($ta -and $ta.LastWriteTime -gt $since -and $ta.LastWriteTime -gt $t0) { $kind = 'traceall'; $t0 = $ta.CreationTime.AddSeconds(-1) }
    # A PLAYING.txt left by a launcher that was closed with its game is stale once no game runs.
    if ((Test-Path $Playing) -and (Running-Games).Count -eq 0) { Remove-Item $Playing -ErrorAction SilentlyContinue }
    if (Test-Path $Playing) { exit 0 }
    # An engine session closed the same way also missed its send (after_session); send it now.
    $game = Get-ChildItem (Logs-Root) -Filter 'game-*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $missedSend = $Config -and $Config.after_session -and (($game -and $game.LastWriteTime -gt $since) -or ($ta -and $ta.LastWriteTime -gt $since))
    # A plain engine session (version '.') closed with Exit game: its panic check too.
    if ($kind -eq '' -and $game -and $game.LastWriteTime -gt $since) { $kind = 'rust'; $t0 = $game.CreationTime.AddSeconds(-1) }
    if ($kind -eq '' -and -not $missedSend) {
        if ($selfSaid) { Save-Result ((Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + "  launcher update`r`n$selfSaid"); exit 10 }
        exit 0
    }
    $summary = ''
    if ($kind) {
        Write-Host '  Checking the last session (it closed before its check)...'
        Start-Stage 'Checking your last session'
        $summary = Check-Session $kind $t0 $null
    }
    if ($missedSend) { Start-Stage 'Sending your last session' }
    if ($missedSend) { $summary = (@($summary, (Send-Sessions)) | Where-Object { $_ }) -join "`r`n" }
    if ($selfSaid) { $summary = "$selfSaid`r`n$summary" }
    Save-Result ((Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + "  (checked at next start)`r`n$summary")
    exit 10
}

# Commands SkateLauncher.exe reads (tab-separated).
if ($Entry -eq 'entries') {
    # id, name, hint, ask (1 = ask for a session-folder label), group, on (1/0), why it is off, info, records ("|")
    $selVer = Get-Version $Version
    foreach ($k in $Entries.Keys) {
        $e = $Entries[$k]
        $ask = 0; if ($e.Recomp -and $e.Recomp.ask_label) { $ask = 1 }
        $why = Entry-Blocked $k $selVer
        $d = $PlayDetails[$k]
        $info = if ($d) { $d.Info } else { '' }
        $records = if ($k -eq 'rust-traceall') { TraceAll-Records $selVer } elseif ($d) { $d.Records } else { @() }
        "{0}`t{1}`t{2}`t{3}`t{4}`t{5}`t{6}`t{7}`t{8}" -f $k, $e.Name, ($e.Hint -replace "`t", ' '), $ask, $e.Group, $(if ($why) { 0 } else { 1 }), $why, $info, ((@($records) | ForEach-Object { $_ -replace '[\t|]', ' ' }) -join '|')
    }
    exit 0
}
if ($Entry -eq 'versions') {
    $sel = (Get-Version '').id
    foreach ($v in $Versions) { "{0}`t{1}`t{2}`t{3}" -f $v.id, $v.name, (Version-Status $v), $(if ($v.id -eq $sel) { '*' } else { '' }) }
    exit 0
}
if ($Entry -eq 'status') {
    $v = Get-Version ''
    "version`t$($v.name)"
    "version_status`t$(Version-Status $v)"
    $last = if (Test-Path $LastSession) { ((Get-Content $LastSession) -join ' / ') } else { 'No session yet.' }
    "last`t$last"
    $sb = Send-Blocked; "send`t$(if (-not (Send-Script)) { 'off' } elseif ($sb) { $sb } else { 'on' })"
    "share`t$(Share-Status (Version-Dir $v))"
    "fixed_executor`t$(Fixed-Executor)"
    "setup_missing`t$(Setup-Missing)"
    "first_setup`t$(if (Setting 'first_setup_done' $false) { '' } else { '1' })"
    # The launcher's own commit and the fork's newest (as of the last check at start), for the main screen.
    $lc = Launcher-Commit; $fc = Fork-Commit
    "launcher`t$(Self-Short $lc)"
    "launcher_newer`t$(if ($fc -and $lc -and $fc -ne $lc) { Self-Short $fc } else { '' })"
    exit 0
}
# build.bat: SkateLauncher.exe from the release (built from this SkateLauncher.cs) or compiled here.
if ($Entry -eq 'launcher-exe') { if (Rebuild-LauncherExe) { 'ok'; exit 0 }; 'failed'; exit 1 }
# The first-time setup's values (the window's wizard): key, value; "standalone" says where versions go by default.
if ($Entry -eq 'config-get') {
    "standalone`t$(if ($Standalone) { '1' } else { '' })"
    "default_root`t$(if ($Standalone) { $Here } else { '' })"
    "storage_root`t$(Setting-Path 'storage_root')"
    "target_root`t$(Setting-Path 'target_root')"
    "build_cap_gb`t$(Setting 'build_cap_gb' 0)"
    "disc`t$(Setting-Path 'disc')"
    "recomp.exe`t$(Setting-Path 'exe' 'recomp')"
    "recomp.working_dir`t$(Setting-Path 'working_dir' 'recomp')"
    "rpcs3.exe`t$(Setting-Path 'exe' 'rpcs3')"
    "rpcs3.game`t$(Setting-Path 'game' 'rpcs3')"
    exit 0
}
# config-set <file>: the wizard's changed values (a JSON object of dotted keys), merged into config.json; the file is removed.
if ($Entry -eq 'config-set') {
    try { Set-ConfigValues $Label; 'saved' } finally { Remove-Item -LiteralPath $Label -ErrorAction SilentlyContinue }
    exit 0
}
if ($Entry -eq 'setup-check') { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here 'setup.ps1'); exit $LASTEXITCODE }
if ($Entry -in @('version-next', 'version-prev', 'version-set')) {
    $cur = [array]::IndexOf(@($Versions | ForEach-Object { $_.id }), (Get-Version '').id)
    if ($Entry -eq 'version-set') { $new = Get-Version $Label }
    else { $step = 1; if ($Entry -eq 'version-prev') { $step = -1 }; $new = $Versions[($cur + $step + $Versions.Count) % $Versions.Count] }
    Set-Content -Path $SelectedFile -Value $new.id -Encoding ASCII
    $Entry = 'notes'; $Label = $new.id
}
if ($Entry -eq 'notes') {
    $v = Get-Version $(if ($Label) { $Label } else { $Version })
    "Version: $($v.name)"
    "Branch: $($v.branch)   Status: $(Version-Status $v)"
    ''
    'What to test:'
    $v.notes
    exit 0
}
if ($Entry -eq 'build') {
    $v = Get-Version $(if ($Label) { $Label } else { $Version })
    if (Ensure-Version $v) { "Ready: $($v.name) ($(Version-Status $v))"; exit 0 } else { "Build failed: $($v.name). Build log: $script:BuildLog"; exit 1 }
}
# Clearing (Tools > Clear builds): a version's build cache is its cargo target folder; its build is that plus bin\
# (the exe it plays), so it builds again at its next launch. Only these launcher-known folders are ever deleted.
if ($Entry -eq 'sizes') {
    # id, name, build cache bytes, bin bytes
    foreach ($v in $Versions) { "{0}`t{1}`t{2}`t{3}" -f $v.id, $v.name, (Folder-Bytes (Target-Dir $v)), (Folder-Bytes (Join-Path (Version-Dir $v) 'bin')) }
    exit 0
}
if ($Entry -in @('clear-cache', 'clear-build')) {
    if ((Running-Games).Count -gt 0) { Save-Result 'Not cleared: a game is running.'; 'Not cleared: a game is running.'; exit 1 }
    $which = @($Versions | Where-Object { $Label -eq 'all' -or $_.id -eq $Label })
    if ($which.Count -eq 0) { "Unknown version: $Label"; exit 1 }
    $freed = [long]0; $done = @()
    foreach ($v in $which) {
        $dirs = @(Target-Dir $v)
        if ($Entry -eq 'clear-build') { $dirs += (Join-Path (Version-Dir $v) 'bin') }
        foreach ($d in $dirs) {
            if (-not (Test-Path -LiteralPath $d)) { continue }
            $freed += Folder-Bytes $d
            Write-Host "  Deleting $d..."
            Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue
        }
        $done += $v.name
    }
    $what = if ($Entry -eq 'clear-build') { 'builds' } else { 'build caches' }
    $r = "Cleared the $what of " + ($done -join ', ') + (" ({0:N1} GB freed)." -f ($freed / 1GB))
    Save-Result ((Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + "  $Entry $Label`r`n$r")
    "  $r"
    exit 0
}
if ($Entry -eq 'room') { Make-Room (Get-Version $(if ($Label) { $Label } else { $Version })); exit 0 }
if ($Entry -eq 'send') { $r = Send-Sessions; if (-not $r) { $r = 'Send: no after_session in config.json.' }; $r; exit 0 }
if ($Entry -in @('fixed-executor-single', 'fixed-executor-multi')) { $o = Read-GameJson 'graphics.json'; Set-Field $o 'physics_threads_multi' ($Entry -eq 'fixed-executor-multi'); Write-GameJson 'graphics.json' $o; exit 0 }
# Tools > Opt-ins and settings: one row per setting (key, group, label, shown value, hint); game-setting <key> steps it.
if ($Entry -eq 'game-settings') {
    $files = @{}
    foreach ($r in $GameRows) {
        if (-not $files.ContainsKey($r[3])) { $files[$r[3]] = Read-GameJson $r[3] }
        $o = $files[$r[3]]
        "{0}`t{1}`t{2}`t{3}`t{4}" -f $r[0], $r[1], $r[2], (Game-Shown $r (Game-Index $r $o) $o), $r[7]
    }
    exit 0
}
if ($Entry -eq 'game-setting') {
    $r = $GameRows | Where-Object { $_[0] -eq $Label } | Select-Object -First 1
    if (-not $r) { Write-Host "Unknown setting: $Label"; exit 1 }
    if (Test-Path $Playing) { Write-Host 'The game is running: change it in the game''s overlay (Guide / F1) instead.'; exit 1 }
    $o = Read-GameJson $r[3]
    $i = Game-Index $r $o
    Set-GameValue $r $o $r[5][($i + 1) % $r[5].Count]
    Write-GameJson $r[3] $o
    exit 0
}
if ($Entry -eq 'share-on') { Share-Run (Version-Dir (Get-Version '')) @('-Consent'); exit 0 }
if ($Entry -eq 'share-off') { Share-Run (Version-Dir (Get-Version '')) @('-Revoke'); exit 0 }
if ($Entry -eq 'list') { $Entries.Keys | ForEach-Object { '{0,-20} {1}' -f $_, $Entries[$_].Name }; exit 0 }
if ($Entry) { Launch $Entry $Label; if ($script:ToolFailed) { exit 1 }; exit 0 }

# No entry: open the window (the menu lives in SkateLauncher.exe now).
$exe = Join-Path $Here 'SkateLauncher.exe'
if (-not (Test-Path $exe)) { Write-Host "SkateLauncher.exe is not built yet: run build.bat in $Here first."; exit 1 }
Start-Process -FilePath $exe
