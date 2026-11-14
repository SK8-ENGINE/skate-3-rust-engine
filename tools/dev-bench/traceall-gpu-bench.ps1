# Frame-time cost of trace-all GPU queries: one muted windowed run per mode, same build, same scene.
# Modes: plain (frame log only), off (trace-all, SKATE_PERF_GPU_EVERY=0), sampled (default 30), full (1).
# Uses its own settings folder (FPS limit off) so the user's graphics.json is never read or written.
param(
    # Default: this checkout's bin (scripts\Build.ps1) and its installation.
    [string]$Bin = '',
    [string]$Assets = '',
    [string]$Work = (Join-Path $env:TEMP 'skate-traceall-gpu-bench'),
    [string[]]$Modes = @('plain', 'off', 'sampled', 'full'),
    [int]$Seconds = 60,
    [int]$SkipSeconds = 25
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot)
if (-not $Bin) { $Bin = Join-Path $repo 'bin' }
if (-not $Assets) {
    # The installation setup made (data\installation.json names its folder).
    $dir = (Get-Content (Join-Path $repo 'data\installation.json') -Raw | ConvertFrom-Json).directory
    $Assets = Join-Path (Join-Path $repo 'data') "$dir\assets"
}
$lock = Join-Path $repo '.local\GAME_LOCK.txt'
New-Item -ItemType Directory -Force -Path (Join-Path $Work 'settings') | Out-Null
$link = Join-Path $Work 'assets'
if (-not (Test-Path $link)) { New-Item -ItemType Junction -Path $link -Target $Assets | Out-Null }
'{"width":1280,"height":800,"scale":100,"fps":0,"hour":12.0,"day_speed":60,"ambient_level":25,"frame_stats":false,"npc_draw_distance":1.0}' |
    Set-Content -Encoding ASCII (Join-Path $Work 'settings\graphics.json')
$exe = (Resolve-Path (Join-Path $Bin 'skate3rust.exe')).Path
$trace = 'SKATE_TRACE_ALL','SKATE_FRAME_LOG','SKATE_AUDIO_STATE_LOG','SKATE_PERF_REPORT','SKATE_PERF_GPU','SKATE_PERF_RENDER','SKATE_GPU_TIMING','SKATE_AUDIO_TRACE','SKATE_AUDIO_TIMING','SKATE_LIVING_WORLD_DEBUG','SKATE_FPS_LOG','SKATE_PERF_GPU_EVERY'
foreach ($mode in $Modes) {
    if (Test-Path (Join-Path $repo '.local\steam-launcher\PLAYING.txt')) { Write-Host 'user is playing: stop'; break }
    if (Test-Path $lock) { Write-Host 'GAME_LOCK exists: stop'; break }
    if (Get-Process skate3rust -ErrorAction SilentlyContinue) { Write-Host 'a game is running: stop'; break }
    $dir = Join-Path $Work $mode
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    foreach ($n in $trace) { Remove-Item "env:$n" -ErrorAction SilentlyContinue }
    $env:SKATE_FRAME_LOG = Join-Path $dir 'frames.tsv'
    if ($mode -ne 'plain') {
        $env:SKATE_TRACE_ALL = '1'
        $env:SKATE_AUDIO_STATE_LOG = Join-Path $dir 'state.tsv'
        $env:SKATE_PERF_REPORT = Join-Path $dir 'perf.json'
        switch ($mode) { 'off' { $env:SKATE_PERF_GPU_EVERY = '0' } 'full' { $env:SKATE_PERF_GPU_EVERY = '1' } }
    }
    "traceall-gpu-bench mode=$mode $(Get-Date -Format s)" | Set-Content -Encoding ASCII $lock
    try {
        $p = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe) -ArgumentList ('--assets "' + $link + '" --mute') `
            -RedirectStandardOutput (Join-Path $dir 'stdout.log') -RedirectStandardError (Join-Path $dir 'stderr.log') -PassThru
        if (-not $p.WaitForExit($Seconds * 1000)) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null }
    } finally { Remove-Item $lock -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 3
    $rows = @(Get-Content (Join-Path $dir 'frames.tsv') | Where-Object { $_ -notmatch '^#|^wall' } | ForEach-Object { ,($_ -split "`t") })
    if ($rows.Count -eq 0) { Write-Host "$mode no frames"; continue }
    $t0 = [double]$rows[0][0]
    $steady = @($rows | Where-Object { [double]$_[0] -ge $t0 + $SkipSeconds })
    $fm = @($steady | ForEach-Object { [double]$_[2] } | Sort-Object)
    $mm = @($steady | ForEach-Object { [double]$_[5] } | Sort-Object)
    $pct = { param($a, $q) $a[[Math]::Min($a.Count - 1, [int][Math]::Ceiling($q * $a.Count) - 1)] }
    "{0,-8} frames={1,5} frame_ms median={2,7:N2} p95={3,7:N2} mean={4,7:N2} | main_ms median={5,6:N2} p95={6,6:N2}" -f $mode, $fm.Count,
        (& $pct $fm 0.5), (& $pct $fm 0.95), (($fm | Measure-Object -Average).Average), (& $pct $mm 0.5), (& $pct $mm 0.95)
}
foreach ($n in $trace) { Remove-Item "env:$n" -ErrorAction SilentlyContinue }
