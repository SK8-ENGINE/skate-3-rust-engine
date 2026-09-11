param([string]$Map)
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
    $argumentList = '--assets "' + $assetsPath + '"'
    if ($Map) {
        $mapPath = (Resolve-Path -LiteralPath $Map).Path
        if ([System.IO.Path]::GetExtension($mapPath) -ine '.skate') { throw 'Select a .skate map file.' }
        $argumentList += ' --map "' + $mapPath + '"'
        Write-Host "Map: $mapPath"
    }
    # Dev packages live in repo mods/; the exe otherwise only looks beside bin/.
    $env:SKATE3_MODS = Join-Path $ProjectRoot 'mods'
    New-Item -ItemType Directory -Path $env:SKATE3_MODS -Force | Out-Null
    Write-Host "Mods: $env:SKATE3_MODS"
    $game = Start-Process -FilePath $executable -WorkingDirectory $ProjectRoot `
        -ArgumentList $argumentList -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $log -RedirectStandardError $errorLog
    if ($game.ExitCode -ne 0) {
        Get-Content -LiteralPath $errorLog -Tail 30
        throw "Game exited with code $($game.ExitCode). Log: $errorLog"
    }
} finally { Pop-Location }
