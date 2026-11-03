# Unattended map switch measurement (needs a build with SKATE_MAP_SWITCH, map_transition.rs): start on -From, switch
# to -To after -After seconds, muted and minimised under the game lock, then print the FRAME_HITCH lines after the
# switch request. Usage: map-switch-run.ps1 -Checkout <dir> [-From University] [-To DownTown] [-After 15] [-Extra <regex>] [-GameArgs <args>] [-KeepLog <path>]
param(
    [Parameter(Mandatory)][string]$Checkout,
    [string]$From = 'University',
    [string]$To = 'DownTown',
    [int]$After = 15,
    # Extra log lines to print (whole run, before and after the switch), as raw lines (regex), e.g. the loaders' summaries.
    [string]$Extra = '',
    # Extra game arguments, e.g. @('--trace', 'K:\...\switch.json').
    [string[]]$GameArgs = @(),
    # Keep the game's stderr log at this path instead of deleting it.
    [string]$KeepLog = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot)
$lock = Join-Path $repo '.local\GAME_LOCK.txt'
if (Get-Process skate3rust -ErrorAction SilentlyContinue) { throw 'A game is already running (one game at a time).' }
if (Test-Path $lock) { throw "Game lock held: $(Get-Content $lock -Raw)" }
Set-Content $lock "map-switch-run $Checkout $(Get-Date -Format s)"
$err = Join-Path $env:TEMP "map-switch-run-$PID.stderr.log"
try {
    $link = Get-Item (Join-Path $Checkout 'assets')
    $assets = if ($link.Target) { @($link.Target)[0] } else { $link.FullName }
    # assets -> data\installations\<id>\assets; the maps sit next to it.
    $maps = Join-Path (Split-Path $assets) 'maps'
    $env:SKATE_REPORT_CHILD = '1'
    $env:SKATE_MAP_SWITCH = "${After}:$maps\$To.skate"
    $p = Start-Process (Join-Path $Checkout 'bin\skate3rust.exe') -ArgumentList (@('--assets', "`"$assets`"", '--map', "`"$maps\$From.skate`"", '--mute') + $GameArgs) `
        -WorkingDirectory (Join-Path $Checkout 'bin') -WindowStyle Minimized -RedirectStandardError $err -PassThru
    $deadline = (Get-Date).AddSeconds($After + 120)
    $committed = $null
    while (-not $p.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        if (-not $committed -and (Select-String -Path $err -Pattern 'MAP_TRANSITION_COMMITTED' -Quiet)) { $committed = Get-Date }
        if ($committed -and ((Get-Date) - $committed).TotalSeconds -ge 10) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null }
    if (-not $committed) { Get-Content $err -Tail 15; throw 'The switch did not commit.' }
    $lines = Get-Content $err | ForEach-Object { $_ -replace '\x1b\[[0-9;]*m', '' }
    $start = ($lines | Select-String -Pattern 'SKATE_MAP_SWITCH requesting' | Select-Object -First 1).LineNumber
    if ($Extra) { $lines | Select-String -Pattern $Extra | ForEach-Object { 'EXTRA ' + ($_.Line -replace '^.*?(?=' + $Extra + ')', '') } }
    $lines | Select-Object -Skip $start | Select-String -Pattern 'MAP_TRANSITION_COMMITTED|MAP_LOAD_TIMING|FRAME_HITCH|PED_DATA_LOAD' |
        ForEach-Object { ($_.Line -replace '.*?(MAP_TRANSITION_COMMITTED|MAP_LOAD_TIMING|FRAME_HITCH|PED_DATA_LOAD)', '$1' -replace ' phases=.*', '') }
} finally {
    Remove-Item env:SKATE_MAP_SWITCH -ErrorAction SilentlyContinue
    Remove-Item $lock -ErrorAction SilentlyContinue
    if ($KeepLog) { Move-Item $err $KeepLog -Force -ErrorAction SilentlyContinue } else { Remove-Item $err -ErrorAction SilentlyContinue }
}
