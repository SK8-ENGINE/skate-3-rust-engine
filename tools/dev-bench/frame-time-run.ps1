# Muted, minimized frame-time run of a checkout's staged release exe (skill regression-check, frame time section).
# Prints median / p90 / p99 frame_ms of frames 100+ from the run's trace-all frames.tsv.
# Usage: frame-time-run.ps1 -Checkout <dir> -Map <map .skate path> [-Seconds 60] [-NoMods] (no mods loaded, like a player
# without mods; by default the checkout's dev mods load) [-KeepLog <path>]
param(
    [Parameter(Mandatory)][string]$Checkout,
    [Parameter(Mandatory)][string]$Map,
    [int]$Seconds = 60,
    [switch]$NoMods,
    # Copy the run's stderr log here before it is deleted.
    [string]$KeepLog = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot)
$lock = Join-Path $repo '.local\GAME_LOCK.txt'
if (Get-Process skate3rust -ErrorAction SilentlyContinue) { throw 'A game is already running (one game at a time).' }
if (Test-Path $lock) { throw "Game lock held: $(Get-Content $lock -Raw)" }
Set-Content $lock "frame-time-run $Checkout $(Get-Date -Format s)"
$err = Join-Path $env:TEMP "frame-time-run-$PID.stderr.log"
try {
    $env:SKATE_TRACE_ALL = '1'
    $env:SKATE_REPORT_CHILD = '1'
    $env:SKATE3_MODS = if ($NoMods) { $empty = Join-Path $env:TEMP 'frame-time-run-nomods'; New-Item -ItemType Directory -Force $empty | Out-Null; $empty } else { Join-Path $Checkout 'mods' }
    $exe = Join-Path $Checkout 'bin\skate3rust.exe'
    $assets = (Get-Item (Join-Path $Checkout 'assets')).FullName
    $p = Start-Process $exe -ArgumentList @('--assets', "`"$assets`"", '--map', "`"$Map`"", '--mute') `
        -WorkingDirectory (Join-Path $Checkout 'bin') -WindowStyle Minimized -RedirectStandardError $err -PassThru
    $deadline = (Get-Date).AddSeconds($Seconds + 90)
    $started = $null
    while (-not $p.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        if (-not $started -and (Select-String -Path $err -Pattern 'TRACE_ACTIVE' -Quiet)) { $started = Get-Date }
        if ($started -and ((Get-Date) - $started).TotalSeconds -ge $Seconds) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null }
    $line = Select-String -Path $err -Pattern 'TRACE_ACTIVE.*' | Select-Object -First 1
    if (-not $line) { Get-Content $err -Tail 20; throw 'No TRACE_ACTIVE line: the run did not start tracing.' }
    # The TRACE_ACTIVE line's paths hold spaces; this run's frames.tsv is the newest one.
    $tsv = Get-ChildItem (Join-Path $Checkout 'bin\logs') -Recurse -Filter frames.tsv | Sort-Object LastWriteTime |
        Select-Object -Last 1 -ExpandProperty FullName
    $rows = Import-Csv $tsv -Delimiter "`t" | Select-Object -Skip 100
    $ms = @($rows | ForEach-Object { [double]$_.frame_ms } | Sort-Object)
    if ($ms.Count -eq 0) { throw "No frames after 100 in $tsv" }
    $q = { param($f) $ms[[Math]::Min($ms.Count - 1, [int][Math]::Floor($f * $ms.Count))] }
    '{0}: {1} frames, median {2:N2} ms, p90 {3:N2} ms, p99 {4:N2} ms ({5})' -f (Split-Path $Map -Leaf), $ms.Count, (& $q 0.5), (& $q 0.9), (& $q 0.99), $tsv
} finally {
    Remove-Item $lock -ErrorAction SilentlyContinue
    if (Test-Path $err) {
        $bad = @(Select-String -CaseSensitive -Path $err -Pattern 'panicked|ERROR|OFFBOARD_INVALID|NONFINITE|physics_failed:true')
        "log check: $($bad.Count) panic/error line(s)"
        $bad | Select-Object -First 5 | ForEach-Object { $_.Line.Substring(0, [Math]::Min(200, $_.Line.Length)) }
        $hitches = @(Select-String -CaseSensitive -Path $err -Pattern 'FRAME_HITCH')
        "hitch lines: $($hitches.Count)"
        $hitches | Select-Object -First 12 | ForEach-Object { $l = $_.Line; $i = $l.IndexOf('FRAME_HITCH'); $l.Substring($i, [Math]::Min(400, $l.Length - $i)) }
        # SYSTEM_TIMING lines (on under trace-all) are kept next to the run's frames.tsv.
        $timing = @(Select-String -CaseSensitive -Path $err -Pattern 'SYSTEM_TIMING frames=' | ForEach-Object { $_.Line.Substring($_.Line.IndexOf('SYSTEM_TIMING')) })
        if ($timing.Count -and $tsv) { $out = Join-Path (Split-Path $tsv) 'system_timing.log'; $timing | Set-Content -Encoding utf8 $out; "system timing: $($timing.Count) line(s) in $out" }
    }
    if ($KeepLog -and (Test-Path $err)) { Copy-Item $err $KeepLog -Force }
    Remove-Item $err -ErrorAction SilentlyContinue
}
