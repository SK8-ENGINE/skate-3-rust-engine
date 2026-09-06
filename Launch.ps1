$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    $executable = Join-Path $PSScriptRoot 'bin/skate-game.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw 'Game is not built. Run BUILD.bat first.' }
    New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot 'logs') -Force | Out-Null
    $log = Join-Path $PSScriptRoot ('logs/game-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
    Write-Host 'Starting imported skater project (Easy). Use your XInput controller; Esc exits.'
    Write-Host "Log: $log"
    $errorLog = [System.IO.Path]::ChangeExtension($log, 'stderr.log')
    $assetArgument = '"' + (Join-Path $PSScriptRoot 'assets') + '"'
    $game = Start-Process -FilePath $executable -WorkingDirectory $PSScriptRoot `
        -ArgumentList @('--assets', $assetArgument) -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $log -RedirectStandardError $errorLog
    if ($game.ExitCode -ne 0) {
        Get-Content -LiteralPath $errorLog -Tail 30
        throw "Game exited with code $($game.ExitCode). Log: $errorLog"
    }
} finally { Pop-Location }
