# Frame-time cost of the Better Water look (settings/water-look.json): one muted windowed run per mode, same build,
# same spot (a teleport to water), idle. Modes: off (retail look), on (Better Water), and on with one piece off.
# Uses its own settings folder (FPS limit off), so the user's graphics.json / water-look.json are never read or written.
# Two rounds by default, the second in reverse order (doc 25 method). Prints frame_ms / main_ms per run and writes
# each run's perf.json (GPU pass times, SKATE_PERF_GPU_EVERY=1) under -Work.
param(
    # Default: this checkout's bin (scripts\Build.ps1) and its installation.
    [string]$Bin = '',
    [string]$Assets = '',
    [string]$Work = (Join-Path $env:TEMP 'skate-water-look-bench'),
    [string]$Teleport = 'dist_downtown_canalplaza',
    [string[]]$Modes = @('off', 'on', 'no_third', 'no_blur', 'no_ripple'),
    [int]$Rounds = 2,
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
'{"width":1280,"height":800,"scale":100,"fps":0,"hour":12.0,"day_speed":0,"ambient_level":25,"frame_stats":false,"npc_draw_distance":1.0}' |
    Set-Content -Encoding ASCII (Join-Path $Work 'settings\graphics.json')
$looks = @{
    off = '{"enabled":false}'
    on = '{"enabled":true}'
    no_third = '{"enabled":true,"third_wave":false}'
    no_blur = '{"enabled":true,"reflection_blur":0.0}'
    no_ripple = '{"enabled":true,"ripple_scale":1.0}'
}
$exe = (Resolve-Path (Join-Path $Bin 'skate3rust.exe')).Path
$trace = 'SKATE_TRACE_ALL','SKATE_FRAME_LOG','SKATE_AUDIO_STATE_LOG','SKATE_PERF_REPORT','SKATE_PERF_GPU','SKATE_PERF_RENDER','SKATE_GPU_TIMING','SKATE_PERF_GPU_EVERY','SKATE_REPORT_CHILD'
$order = @()
for ($r = 1; $r -le $Rounds; $r++) {
    $round = [string[]]$Modes.Clone()
    if ($r % 2 -eq 0) { [array]::Reverse($round) }
    foreach ($m in $round) { $order += "$m#$r" }
}
foreach ($run in $order) {
    $mode = $run.Split('#')[0]
    if (Test-Path (Join-Path $repo '.local\steam-launcher\PLAYING.txt')) { Write-Host 'user is playing: stop'; break }
    if (Test-Path $lock) { Write-Host 'GAME_LOCK exists: stop'; break }
    if (Get-Process skate3rust, skate3 -ErrorAction SilentlyContinue) { Write-Host 'a game is running: stop'; break }
    $dir = Join-Path $Work ($run -replace '#', '-r')
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $looks[$mode] | Set-Content -Encoding ASCII (Join-Path $Work 'settings\water-look.json')
    foreach ($n in $trace) { Remove-Item "env:$n" -ErrorAction SilentlyContinue }
    $env:SKATE_REPORT_CHILD = '1'
    $env:SKATE_TRACE_ALL = '1'
    $env:SKATE_FRAME_LOG = Join-Path $dir 'frames.tsv'
    $env:SKATE_AUDIO_STATE_LOG = Join-Path $dir 'state.tsv'
    $env:SKATE_PERF_REPORT = Join-Path $dir 'perf.json'
    $env:SKATE_PERF_GPU_EVERY = '1'
    "water-look-bench run=$run $(Get-Date -Format s)" | Set-Content -Encoding ASCII $lock
    try {
        $p = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe) -ArgumentList ('--assets "' + $link + '" --mute --teleport ' + $Teleport) `
            -RedirectStandardOutput (Join-Path $dir 'stdout.log') -RedirectStandardError (Join-Path $dir 'stderr.log') -PassThru
        if (-not $p.WaitForExit($Seconds * 1000)) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null }
    } finally { Remove-Item $lock -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 3
    $rows = @(Get-Content (Join-Path $dir 'frames.tsv') -ErrorAction SilentlyContinue | Where-Object { $_ -notmatch '^#|^wall' } | ForEach-Object { ,($_ -split "`t") })
    if ($rows.Count -eq 0) { Write-Host "$run no frames"; continue }
    $t0 = [double]$rows[0][0]
    $steady = @($rows | Where-Object { [double]$_[0] -ge $t0 + $SkipSeconds })
    $fm = @($steady | ForEach-Object { [double]$_[2] } | Sort-Object)
    $mm = @($steady | ForEach-Object { [double]$_[5] } | Sort-Object)
    $pct = { param($a, $q) $a[[Math]::Min($a.Count - 1, [int][Math]::Ceiling($q * $a.Count) - 1)] }
    "{0,-12} frames={1,5} frame_ms p50={2,6:N2} p90={3,6:N2} p99={4,6:N2} | main_ms p50={5,6:N2}" -f $run, $fm.Count,
        (& $pct $fm 0.5), (& $pct $fm 0.9), (& $pct $fm 0.99), (& $pct $mm 0.5)
}
foreach ($n in $trace) { Remove-Item "env:$n" -ErrorAction SilentlyContinue }
