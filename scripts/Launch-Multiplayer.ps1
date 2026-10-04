param(
    [ValidateRange(1, 10)][int]$Instances = 2,
    [string]$Server = '127.0.0.1:31030',
    [switch]$NoServer,
    [string]$GameDirectory,
    [string]$Assets,
    [string]$Map,
    [switch]$TestWorld,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'

if (-not $GameDirectory) {
    $GameDirectory = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'skate3rust.exe')) {
        $PSScriptRoot
    } else {
        Join-Path (Split-Path $PSScriptRoot -Parent) 'dist'
    }
}
$GameDirectory = (Resolve-Path -LiteralPath $GameDirectory).Path
$gameExe = Join-Path $GameDirectory 'skate3rust.exe'
$serverExe = Join-Path $GameDirectory 'skated.exe'
if (-not (Test-Path -LiteralPath $gameExe)) { throw 'Game is not built. Run scripts/build-multiplayer.ps1 first.' }
if (-not $NoServer -and -not (Test-Path -LiteralPath $serverExe)) { throw "Session server not found: $serverExe" }
if (-not $Assets) {
    $Assets = Join-Path $GameDirectory 'assets'
    if (-not (Test-Path -LiteralPath $Assets) -and $env:SKATE3_ASSETS) { $Assets = $env:SKATE3_ASSETS }
}
$Assets = (Resolve-Path -LiteralPath $Assets).Path
if ($Map -and $TestWorld) { throw 'Choose -Map or -TestWorld, not both.' }
if ($Map) { $Map = (Resolve-Path -LiteralPath $Map).Path }

# Start-Process joins arrays without preserving embedded spaces on Windows
# PowerShell 5.1. Quote each argument according to the Windows argv rules.
function Quote-Argument([string]$Value) {
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    '"' + [regex]::Replace($escaped, '(\\+)$', '$1$1') + '"'
}

$run = (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$logs = Join-Path $GameDirectory "logs/multiplayer-$run"
Write-Host "Local multiplayer: $Instances client(s), all using Vulkan."
Write-Host 'Rendering: 30 FPS focused / 15 FPS background, 50% scale, 256px world textures. Physics/networking stay active.'
Write-Host "Logs: $logs"
if (-not $DryRun) { New-Item -ItemType Directory -Path $logs -Force | Out-Null }

$serverProcess = $null
$games = @()
try {
if (-not $NoServer) {
    # Synchronous shader compilation can temporarily stall the first update.
    $serverArgs = '-bind ' + (Quote-Argument $Server) + ' -timeout 30s'
    if ($DryRun) {
        Write-Host "SERVER: $(Quote-Argument $serverExe) $serverArgs"
    } else {
        $serverProcess = Start-Process -FilePath $serverExe -WorkingDirectory $GameDirectory -ArgumentList $serverArgs `
            -RedirectStandardOutput (Join-Path $logs 'server.log') -RedirectStandardError (Join-Path $logs 'server.stderr.log') -PassThru
        Start-Sleep -Milliseconds 500
        if ($serverProcess.HasExited) { throw "Server exited. If it is already running, use --no-server. Log: $logs/server.stderr.log" }
        Write-Host "Server PID: $($serverProcess.Id)"
    }
}

for ($index = 1; $index -le $Instances; $index++) {
    # Bound offsets to the engine's existing 20-metre CLI limit.
    $offset = (($index - 1) % 10) * 2
    $clientArgs = @('--assets', $Assets, '--net-server', $Server, '--gpu-backend', 'vulkan',
        '--multi-instance', '--player-title', "Skate MP - client $index", '--spawn-offset', "$offset")
    if ($Map) { $clientArgs += @('--map', $Map) }
    if ($TestWorld) { $clientArgs += '--test-world' }
    $arguments = ($clientArgs | ForEach-Object { Quote-Argument $_ }) -join ' '
    if ($DryRun) {
        Write-Host "CLIENT ${index}: $(Quote-Argument $gameExe) $arguments"
        continue
    }
    $errorLog = Join-Path $logs "client-$index.stderr.log"
    $game = Start-Process -FilePath $gameExe -WorkingDirectory $GameDirectory -ArgumentList $arguments `
        -RedirectStandardOutput (Join-Path $logs "client-$index.log") -RedirectStandardError $errorLog -PassThru
    $games += $game
    Write-Host "Client $index PID: $($game.Id)"

    # Loading large maps and compiling pipelines can exceed a fixed five-second
    # delay. Wait for a simulation tick before starting the next GPU workload.
    $deadline = [DateTime]::UtcNow.AddMinutes(3)
    while ($true) {
        $tail = if (Test-Path -LiteralPath $errorLog) { (Get-Content -LiteralPath $errorLog -Tail 200) -join "`n" } else { '' }
        if ($game.HasExited -or $tail -match 'REPORT_PANIC|REPORT_NATIVE|Crash report saved:|memory allocation .* failed') { throw "Client $index failed to start. Log: $errorLog" }
        if ($tail -match 'REPORT_META state=.*physics_tick:[1-9][0-9]*') { break }
        if ([DateTime]::UtcNow -ge $deadline) { throw "Client $index startup timed out. Log: $errorLog" }
        Start-Sleep -Milliseconds 500
    }
    if ($index -lt $Instances) { Start-Sleep -Seconds 2 }
}
if (-not $DryRun) {
    Write-Host 'Clients launched. Close all game windows when done; the server started here will stop automatically.'
    while (@($games | Where-Object { -not $_.HasExited }).Count -gt 0) {
        Start-Sleep -Seconds 1
    }
}
} finally {
    # Only terminate processes created by this invocation. In particular,
    # -NoServer must never stop an existing server or someone else's clients.
    foreach ($game in $games) {
        if (-not $game.HasExited) {
            # Include this client's crash supervisor/child, not unrelated games.
            & taskkill.exe /PID $game.Id /T /F 2>&1 | Out-Null
        }
    }
    if ($serverProcess -and -not $serverProcess.HasExited) {
        Stop-Process -Id $serverProcess.Id -ErrorAction SilentlyContinue
    }
}
