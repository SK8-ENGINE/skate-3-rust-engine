param([string]$Map)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    $executable = Join-Path $PSScriptRoot 'bin/skate3rust.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw 'Game is not built. Run BUILD.bat first.' }
    New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot 'logs') -Force | Out-Null
    $log = Join-Path $PSScriptRoot ('logs/game-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
    Write-Host 'Starting Skate 3 Rust Engine. Use your XInput controller; Esc opens difficulty/graphics/pause settings.'
    Write-Host "Log: $log"
    $errorLog = [System.IO.Path]::ChangeExtension($log, 'stderr.log')
    $assetArgument = '"' + (Join-Path $PSScriptRoot 'assets') + '"'
    $gameArguments = @('--assets', $assetArgument)
    if ($Map) {
        $mapPath = (Resolve-Path -LiteralPath $Map).Path
        if ([System.IO.Path]::GetExtension($mapPath) -ine '.skate') { throw 'Select a .skate map file.' }
        $gameArguments += @('--map', ('"' + $mapPath + '"'))
        Write-Host "Map: $mapPath"
    }
    $game = Start-Process -FilePath $executable -WorkingDirectory $PSScriptRoot `
        -ArgumentList $gameArguments -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $log -RedirectStandardError $errorLog
    if ($game.ExitCode -ne 0) {
        Get-Content -LiteralPath $errorLog -Tail 30
        throw "Game exited with code $($game.ExitCode). Log: $errorLog"
    }
} finally { Pop-Location }
