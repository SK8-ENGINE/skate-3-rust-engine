# Automated in-game check of the audio modding features (PRs #36 + #43) with the dev mod
# mods/audio-content-test and its `autotest` setting: nobody presses keys.
#
#   audio_mod_autotest.ps1 [-Worktree <checkout with bin\skate3rust.exe>] [-Pass on|off|both]
#                          [-Audible] [-StepSeconds N] [-GapSeconds N] [-TimeoutSeconds N] [-WaitMinutes 30]
#
# Pass "on":  DownTown, the dev mod enabled with autotest on (a copy of the mod in a run folder and a
#             private SKATE3_MOD_SETTINGS folder, so neither the checkout's mod nor your saved mod
#             settings are touched). The mod logs one `AUTOTEST <check> ok|fail <details>` line per check;
#             this script answers its hot-swap requests (edits / reverts the copy's audio.json) and stops
#             the game after `AUTOTEST done`.
# Pass "off": DownTown, the dev mod present but disabled: asserts retail (map audio ["retail"], the
#             retail crossfade, no overlay, no mod log lines, the install's project count).
# Default: muted (--mute), window minimized, 5 s steps, no gaps. -Audible: unmuted, a normal window,
# 15 s steps with a 3 s quiet gap before each (the HUD counts down: "Next in 3 s: ..." then
# "3/16 <step>  9 s left" and what to listen for). -StepSeconds / -GapSeconds override either; the
# hard timeout follows from them. Silent read-back steps (F5 global, seed, Csis, hot swap) always run
# 3 s (mod setting autotest_quiet_step) after a gap of at most 1 s. The camera is parked at DownTown's
# Baby_Cry_1 emitter only for steps 1-3 (the HUD says so) and returns to the skater near the end of
# step 3 (check camera_back), on a step timeout, at the end and when the mod stops.
# The run stops early when the mod fails (a Lua error stops it) or a step times out in the mod.
# Launch rules: one game at a time. It does not start while any skate3 / skate3rust runs, or while
# .local\GAME_LOCK.txt exists (other tools can write it to claim the game): it waits (60 s checks,
# -WaitMinutes limit). It writes GAME_LOCK.txt while its game runs and deletes only its own lock.
# Output: .local\autotest\runs\<stamp>-<pass>\ (stderr.txt = the game log, results.txt).
# Exit code: 0 all passes ok, 1 a check failed, 2 not launched (busy / timeout waiting), 3 setup error.
param(
    [string]$Worktree = '',
    [ValidateSet('on', 'off', 'both')] [string]$Pass = 'both',
    [switch]$Audible,
    [double]$StepSeconds = 0,
    [double]$GapSeconds = -1,
    [int]$TimeoutSeconds = 0,
    [int]$OffSeconds = 25,
    [int]$WaitMinutes = 30,
    [string]$Map = 'DownTown'
)
$ErrorActionPreference = 'Stop'
$ModId = 'dev-audio-content-test'
if ($StepSeconds -le 0) { $StepSeconds = if ($Audible) { 15 } else { 5 } }
if ($GapSeconds -lt 0) { $GapSeconds = if ($Audible) { 3 } else { 0 } }
$StepCount = 16
if ($TimeoutSeconds -le 0) { $TimeoutSeconds = [int](90 + $StepCount * ($StepSeconds + $GapSeconds) + 40) }

# The main checkout: this script lives in <main>\.claude\skills\audio-autotest\tools (or anywhere below a
# checkout with Cargo.toml at its root). Runs and the lock go to <main>\.local.
$main = $PSScriptRoot
while ($main -and -not (Test-Path -LiteralPath (Join-Path $main 'Cargo.toml'))) { $main = Split-Path $main -Parent }
if (-not $main) { Write-Host 'run this script from inside a checkout'; exit 3 }
if (-not $Worktree) { $Worktree = $main }
$Worktree = (Resolve-Path -LiteralPath $Worktree).Path
$exe = Join-Path $Worktree 'bin\skate3rust.exe'
$modSrc = Join-Path $Worktree 'mods\audio-content-test'
$assets = Join-Path $Worktree 'assets'
if (-not (Test-Path -LiteralPath $exe)) { Write-Host "no game at $exe"; exit 3 }
if (-not (Test-Path -LiteralPath (Join-Path $modSrc 'main.lua'))) { Write-Host "no dev mod at $modSrc"; exit 3 }
$target = (Get-Item -LiteralPath $assets).Target
if ($target -is [array]) { $target = $target[0] }
if (-not $target) { $target = $assets }
$mapPath = Join-Path (Split-Path $target -Parent) "maps\$Map.skate"
if (-not (Test-Path -LiteralPath $mapPath)) { Write-Host "no map at $mapPath"; exit 3 }
$lock = Join-Path $main '.local\GAME_LOCK.txt'
$esc = [char]27

function Busy {
    $p = Get-Process -Name skate3, skate3rust -ErrorAction SilentlyContinue
    if ($p) { return "a game runs (pid $(@($p | ForEach-Object Id) -join ','))" }
    if (Test-Path -LiteralPath $lock) { return "GAME_LOCK.txt exists: $((Get-Content -LiteralPath $lock -Raw).Trim())" }
    return $null
}

function Wait-Free {
    $deadline = (Get-Date).AddMinutes($WaitMinutes)
    while ($true) {
        $why = Busy
        if (-not $why) { return $true }
        if ((Get-Date) -ge $deadline) { Write-Host "not launching: $why (waited $WaitMinutes min)"; return $false }
        Write-Host "waiting: $why"
        Start-Sleep -Seconds 60
    }
}

function Clean([string]$line) { return ($line -replace "$esc\[[0-9;]*m", '') }

function Run-Pass([string]$name) {
    $stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
    $run = Join-Path $main ".local\autotest\runs\$stamp-$name"
    $mods = Join-Path $run 'mods'
    $settings = Join-Path $run 'settings'
    New-Item -ItemType Directory -Force -Path $mods, $settings | Out-Null
    Copy-Item -LiteralPath $modSrc -Destination (Join-Path $mods 'audio-content-test') -Recurse
    Get-ChildItem -LiteralPath (Join-Path $mods 'audio-content-test') -Recurse -Filter '__pycache__' -Directory | Remove-Item -Recurse -Force
    $audioJson = Join-Path $mods 'audio-content-test\audio.json'
    $original = [System.IO.File]::ReadAllBytes($audioJson)
    $audible = $Audible -and $name -eq 'on'
    if ($name -eq 'on') {
        $pref = @{ enabled = $true; values = @{ autotest = $true; autotest_step = $StepSeconds; autotest_gap = $GapSeconds; autotest_muted = (-not $audible); events = $true } } | ConvertTo-Json -Compress
        [System.IO.File]::WriteAllText((Join-Path $settings "$ModId.json"), $pref)
    }
    $err = Join-Path $run 'stderr.txt'
    $out = Join-Path $run 'stdout.txt'
    $argv = @('--assets', $assets, '--map', $mapPath)
    if (-not $audible) { $argv += '--mute' }

    if (-not (Wait-Free)) { return @{ name = $name; launched = $false; ok = $false; run = $run } }
    $env:SKATE_REPORT_CHILD = '1'
    $env:SKATE3_MODS = $mods
    $env:SKATE3_MOD_SETTINGS = $settings
    Remove-Item Env:SKATE3_MODS_ENABLE -ErrorAction SilentlyContinue
    $style = if ($audible) { 'Normal' } else { 'Minimized' }
    $lockText = "autotest-agent $(Get-Date -Format s) pass=$name"
    [System.IO.File]::WriteAllText($lock, $lockText)
    $p = $null
    $edited = $false
    $answered = @{}
    $started = Get-Date
    $doneAt = $null
    $readyAt = $null
    try {
        $p = Start-Process -FilePath $exe -ArgumentList ($argv | ForEach-Object { '"' + $_ + '"' }) -WorkingDirectory $Worktree -PassThru `
            -WindowStyle $style -RedirectStandardOutput $out -RedirectStandardError $err
        Write-Host "pass $name`: pid $($p.Id), $(if ($audible) { 'AUDIBLE' } else { 'muted' }), steps $StepSeconds s + gaps $GapSeconds s, timeout $TimeoutSeconds s, log $err"
        while (-not $p.HasExited) {
            Start-Sleep -Milliseconds 250
            $elapsed = ((Get-Date) - $started).TotalSeconds
            if ($elapsed -gt $TimeoutSeconds) { Write-Host "pass $name`: hard timeout $TimeoutSeconds s"; break }
            $text = ''
            try {
                $fs = [System.IO.File]::Open($err, 'Open', 'Read', 'ReadWrite')
                $sr = New-Object System.IO.StreamReader($fs)
                $text = $sr.ReadToEnd(); $sr.Close()
            } catch { continue }
            if ($name -eq 'on' -and $text -match "mod $ModId stopped|main\.lua:\d+:") {
                Write-Host "pass on: the mod failed (see the log); stopping"
                break
            }
            if ($name -eq 'on') {
                if (-not $answered.edit -and $text -match 'AUTOTEST_REQ edit_audio_json') {
                    $s = [System.Text.Encoding]::UTF8.GetString($original)
                    $s2 = $s.Replace('"volume": 0.5, "time_a"', '"volume": 0.45, "time_a"')
                    if ($s2 -eq $s) { Write-Host 'hot-swap edit: pattern not found in audio.json' }
                    [System.IO.File]::WriteAllText($audioJson, $s2)
                    $answered.edit = $true; $edited = $true
                    Write-Host "pass on: edited audio.json (int_tunnel volume 0.5 -> 0.45) at $([int]$elapsed) s"
                }
                if (-not $answered.revert -and $text -match 'AUTOTEST_REQ revert_audio_json') {
                    [System.IO.File]::WriteAllBytes($audioJson, $original)
                    $answered.revert = $true; $edited = $false
                    Write-Host "pass on: reverted audio.json at $([int]$elapsed) s"
                }
                if (-not $doneAt -and $text -match 'AUTOTEST done') { $doneAt = Get-Date }
                if ($doneAt -and ((Get-Date) - $doneAt).TotalSeconds -ge 1.5) { break }
            } else {
                if (-not $readyAt -and $text -match "Map audio $Map`:") { $readyAt = Get-Date }
                if ($readyAt -and ((Get-Date) - $readyAt).TotalSeconds -ge $OffSeconds) { break }
            }
        }
    } finally {
        if ($p -and -not $p.HasExited) {
            [void]$p.CloseMainWindow()
            if (-not $p.WaitForExit(5000)) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; [void]$p.WaitForExit(5000) }
        }
        if ($edited) { [System.IO.File]::WriteAllBytes($audioJson, $original) }
        if ((Test-Path -LiteralPath $lock) -and ((Get-Content -LiteralPath $lock -Raw).Trim() -eq $lockText)) { Remove-Item -LiteralPath $lock -Force }
        Remove-Item Env:SKATE3_MODS, Env:SKATE3_MOD_SETTINGS -ErrorAction SilentlyContinue
    }
    Start-Sleep -Seconds 1
    $left = Get-Process -Name skate3rust -ErrorAction SilentlyContinue
    $lines = @(Get-Content -LiteralPath $err | ForEach-Object { Clean $_ })
    return @{ name = $name; launched = $true; run = $run; lines = $lines; leftover = [bool]$left; seconds = [int]((Get-Date) - $started).TotalSeconds }
}

# Every check the mod makes, in order (a missing one is a failure).
$expected = @('start', 'babycry_emitter', 'F6_post', 'F7_release', 'camera_back', 'F5_global_set', 'F5_global_restore', 'F8_native_siren', 'F8_native_stop',
    'F9_tuning_write', 'F9_tuning_restore', 'F10_emitter_zone', 'F10_removed', 'F11_rules_set', 'F11_landing_seen', 'F11_rules_removed',
    'D1_duck', 'D1_duck_release', 'D2_seed', 'D2_seed_release', 'D3_own_taxis', 'D3_taxis_removed', 'D4_csis', 'D4_csis_release',
    'D5_nose_rule', 'D5_pop_seen', 'D5_nose_removed', 'D6_orbit', 'D6_orbit_moving', 'D6_orbit_removed', 'tags_after_resubscribe',
    'L7_hot_swap_edit', 'L7_hot_swap_revert', 'no_restart')

function Common([hashtable]$r, [System.Collections.ArrayList]$rows) {
    $panics = @($r.lines | Where-Object { $_ -match 'panicked' }).Count
    [void]$rows.Add([pscustomobject]@{ check = 'no_panic'; result = $(if ($panics -eq 0) { 'ok' } else { 'fail' }); details = "panic lines=$panics" })
    [void]$rows.Add([pscustomobject]@{ check = 'stopped_clean'; result = $(if (-not $r.leftover) { 'ok' } else { 'fail' }); details = "leftover skate3rust=$($r.leftover), $($r.seconds) s" })
    $mapLine = $r.lines | Where-Object { $_ -match "Map audio $Map`:" } | Select-Object -Last 1
    $proj = $r.lines | Where-Object { $_ -match 'native AEMS runtime on \((\d+) projects\)' } | Select-Object -Last 1
    $mapInfo = if ($mapLine) { ($mapLine -replace '^.*?Map audio ', 'Map audio ') } else { 'none' }
    $projInfo = if ($proj) { ($proj -replace '^.*runtime on ', '') } else { 'none' }
    return @{ map = $mapInfo; projects = $projInfo }
}

$results = @()
$passes = if ($Pass -eq 'both') { @('on', 'off') } else { @($Pass) }
$allOk = $true
foreach ($name in $passes) {
    $r = Run-Pass $name
    if (-not $r.launched) { Write-Host "pass $name not launched"; exit 2 }
    $rows = New-Object System.Collections.ArrayList
    $c = Common $r $rows
    $audible = $Audible -and $name -eq 'on'
    if ($name -eq 'on') {
        $seen = @{}
        foreach ($l in $r.lines) {
            if ($l -match "Lua \[$ModId\]: AUTOTEST (\S+) (ok|fail) ?(.*)$") {
                $seen[$Matches[1]] = $true
                [void]$rows.Add([pscustomobject]@{ check = $Matches[1]; result = $Matches[2]; details = $Matches[3] })
            }
        }
        foreach ($e in $expected) { if (-not $seen.ContainsKey($e)) { [void]$rows.Add([pscustomobject]@{ check = $e; result = 'fail'; details = 'missing (never logged)' }) } }
        $loads = @($r.lines | Where-Object { $_ -match "Lua \[$ModId\]: AUTOTEST_INFO on:" }).Count
        [void]$rows.Add([pscustomobject]@{ check = 'script_loaded_once'; result = $(if ($loads -eq 1) { 'ok' } else { 'fail' }); details = "on_load runs=$loads" })
        $ok = $c.map -match 'crossfade Some\("DEV_fade"\)'
        [void]$rows.Add([pscustomobject]@{ check = 'map_audio_mod'; result = $(if ($ok) { 'ok' } else { 'fail' }); details = "$($c.map); $($c.projects)" })
        $cry = $r.lines | Where-Object { $_ -match 'AUDIO_EMITTER start Baby_Cry_1 .*\(native\)' } | Select-Object -First 1
        # audio.json replaces 4 banks: Baby_Cry_1 (bell), Buoy_Bell (chime), Transformer_Lrg_left_2 (triangle), water_lapping_pond (shaker).
        $repl = $r.lines | Where-Object { $_ -match "mod $ModId audio content: replaced banks: 4" } | Select-Object -First 1
        [void]$rows.Add([pscustomobject]@{ check = 'babycry_map_record_started'; result = $(if ($cry -and $repl) { 'ok' } else { 'fail' }); details = $(if ($cry) { ($cry -replace '^.*AUDIO_EMITTER', 'AUDIO_EMITTER') + ' (the map record; Baby_Cry_1 is the bank audio.json replaces)' } else { 'no map-record AUDIO_EMITTER start of Baby_Cry_1' }) })
        # The HUD's countdown (logged at each step / gap start) and the camera's return.
        $hud = @($r.lines | Where-Object { $_ -match "Lua \[$ModId\]: AUTOTEST_HUD \d+/\d+ .* s left" }).Count
        $back = $r.lines | Where-Object { $_ -match "Lua \[$ModId\]: AUTOTEST_INFO camera back at the skater" } | Select-Object -First 1
        [void]$rows.Add([pscustomobject]@{ check = 'hud_countdown'; result = $(if ($hud -ge 16) { 'ok' } else { 'fail' }); details = "AUTOTEST_HUD step lines with a countdown=$hud (want >= 16)" })
        [void]$rows.Add([pscustomobject]@{ check = 'camera_returned'; result = $(if ($back) { 'ok' } else { 'fail' }); details = $(if ($back) { $back -replace '^.*AUTOTEST_INFO ', '' } else { 'no camera return line' }) })
        $done = $r.lines | Where-Object { $_ -match 'AUTOTEST done' } | Select-Object -Last 1
        [void]$rows.Add([pscustomobject]@{ check = 'done'; result = $(if ($done) { 'ok' } else { 'fail' }); details = $(if ($done) { $done -replace '^.*AUTOTEST ', '' } else { 'no done line (timeout?)' }) })
    } else {
        $retail = $c.map -match [regex]::Escape('["retail"]') -and $c.map -match 'crossfade Some\("Main_Ambience_Crossfade_DT"\)'
        [void]$rows.Add([pscustomobject]@{ check = 'map_audio_retail'; result = $(if ($retail) { 'ok' } else { 'fail' }); details = $c.map })
        $modLines = @($r.lines | Where-Object { $_ -match "Lua \[$ModId\]|mod $ModId audio content|$ModId" }).Count
        [void]$rows.Add([pscustomobject]@{ check = 'no_mod_activity'; result = $(if ($modLines -eq 0) { 'ok' } else { 'fail' }); details = "lines naming the mod=$modLines" })
        $swaps = @($r.lines | Where-Object { $_ -match 'swapped the mods|restarted for the mods' }).Count
        [void]$rows.Add([pscustomobject]@{ check = 'no_overlay'; result = $(if ($swaps -eq 0 -and $c.projects -match '^\(?9 projects') { 'ok' } else { 'fail' }); details = "content swaps/restarts=$swaps, $($c.projects) (install: 9)" })
    }
    $fails = @($rows | Where-Object { $_.result -ne 'ok' }).Count
    if ($fails) { $allOk = $false }
    $table = $rows | Format-Table -AutoSize -Wrap check, result, details | Out-String -Width 400
    $summary = "pass $name`: $(@($rows).Count - $fails) ok, $fails fail ($($r.run))"
    Set-Content -LiteralPath (Join-Path $r.run 'results.txt') -Value ($table + $summary) -Encoding utf8
    Write-Host $table
    Write-Host $summary
}
if ($allOk) { exit 0 } else { exit 1 }
