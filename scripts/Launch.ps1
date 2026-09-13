param([string]$Map, [switch]$Trace, [string]$TraceFile)
$ProjectRoot = Split-Path $PSScriptRoot -Parent
$ErrorActionPreference = 'Stop'
Push-Location $ProjectRoot
try {
    $executable = Join-Path $ProjectRoot 'bin/skate3rust.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw 'Game is not built. Run BUILD.bat first.' }
    New-Item -ItemType Directory -Path (Join-Path $ProjectRoot 'logs') -Force | Out-Null
    $log = Join-Path $ProjectRoot ('logs/game-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
    Write-Host 'Starting Skate 3 Rust Engine. Use your XInput controller; Esc opens difficulty/graphics/pause settings.'
    Write-Host "Log: $log"
    $errorLog = [System.IO.Path]::ChangeExtension($log, 'stderr.log')
    # Start-Process ArgumentList arrays split on spaces on Windows PowerShell 5.1.
    # Pass one quoted command-line string so paths under "Ethans Desktop 2.0" stay intact.
    $assetsPath = Join-Path $ProjectRoot 'assets'
    if (-not (Test-Path -LiteralPath $assetsPath)) {
        throw @"
Prepared assets folder is missing:
  $assetsPath

Dev launch expects converted Skate 3 assets there (often via junction/symlink to an installed copy's assets).
Release packages run setup from support\skate3setup.exe instead of --assets.
"@
    }
    try {
        $null = [System.IO.Directory]::EnumerateFileSystemEntries($assetsPath)
    } catch {
        $item = Get-Item -LiteralPath $assetsPath -Force
        $target = if ($item.LinkType) { $item.Target } else { $assetsPath }
        throw @"
Assets path exists but cannot be opened (broken junction or missing target):
  $assetsPath
  -> $target

Re-point assets to a valid installation, for example:
  rmdir "$assetsPath"
  mklink /J "$assetsPath" "C:\path\to\skate3rust-windows-x64\data\installations\<id>\assets"
"@
    }
    $argumentList = '--assets "' + $assetsPath + '"'
    if (-not $Map -and $env:SKATE_TRACE_MAP) { $Map = $env:SKATE_TRACE_MAP }
    if (-not $TraceFile -and $env:SKATE_TRACE_FILE) { $TraceFile = $env:SKATE_TRACE_FILE }
    if (-not $Map -and $Trace) {
        $installRoot = Split-Path (Resolve-Path -LiteralPath $assetsPath).Path -Parent
        foreach ($candidate in @(
            (Join-Path $installRoot 'maps\DownTown.skate'),
            (Join-Path $assetsPath 'private\native-backdrops\DownTown.skate')
        )) {
            if (Test-Path -LiteralPath $candidate) { $Map = $candidate; break }
        }
    }
    if ($Map) {
        $mapPath = (Resolve-Path -LiteralPath $Map).Path
        if ([System.IO.Path]::GetExtension($mapPath) -ine '.skate') { throw "Not a .skate map file: $mapPath" }
        $argumentList += ' --map "' + $mapPath + '"'
        Write-Host "Map: $mapPath"
    }
    if ($Trace) {
        $tracePath = if ($TraceFile) {
            if ([System.IO.Path]::IsPathRooted($TraceFile)) { $TraceFile } else { Join-Path $ProjectRoot $TraceFile }
        } else {
            Join-Path $ProjectRoot 'trace-downtown-lag.json'
        }
        if (Test-Path -LiteralPath $tracePath) { Remove-Item -LiteralPath $tracePath -Force }
        $argumentList += ' --trace "' + $tracePath + '" --trace-wait --trace-gpu'
        Write-Host "Trace: $tracePath"
        Write-Host 'F9 = start recording at lag spot, F10 = stop and export (open in https://ui.perfetto.dev)'
    }
    # Dev packages live in repo mods/; the exe otherwise only looks beside bin/.
    $env:SKATE3_MODS = Join-Path $ProjectRoot 'mods'
    New-Item -ItemType Directory -Path $env:SKATE3_MODS -Force | Out-Null
    Write-Host "Mods: $env:SKATE3_MODS"
    if ($Trace) {
        # Use cmd start so the game gets a real window; PS Start-Process argument quoting is flaky.
        $cmd = 'start "Skate3 Rust" /D "' + $ProjectRoot + '" "' + $executable + '" ' + $argumentList
        Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $cmd -WorkingDirectory $ProjectRoot
        Write-Host 'Game launched in its own window. Close it when you are done.'
        return
    }
    $game = Start-Process -FilePath $executable -WorkingDirectory $ProjectRoot `
        -ArgumentList $argumentList -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $log -RedirectStandardError $errorLog
    if ($game.ExitCode -ne 0) {
        Get-Content -LiteralPath $errorLog -Tail 30
        throw "Game exited with code $($game.ExitCode). Log: $errorLog"
    }
} finally { Pop-Location }
