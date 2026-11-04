# Main PC: put the traced recomp build (skate3.exe + its runtime DLL) in the second PC's share, and on the first
# time also the recomp game folder, so play-recomp.ps1 on the second PC can copy them (-Update / -GetGame).
# They hold recompiled game code and your game files: they go to your own share only, never to GitHub.
# Guide: docs/hails-additions/second-pc-testing.md#recomp-trace-sessions
param(
    # The recomp build folder holding skate3.exe (built from the research-hooks branch).
    [Parameter(Mandatory = $true)][string]$Build,
    # The shared folder (the one setup-main-pc.ps1 shared as SkateSessions).
    [Parameter(Mandatory = $true)][string]$Folder,
    # The recomp game folder you play from (holds game\ and dlc\); with -WithGame it is staged once.
    [string]$Game = '',
    [switch]$WithGame,
    # Also stage this PC's recomp save (%APPDATA%\skate3: profile, settings, downloaded content; not the shader
    # cache or backups), so the second PC plays the same career (play-recomp.ps1 -GetSave).
    [switch]$WithSave
)
$ErrorActionPreference = 'Stop'
$exe = Join-Path $Build 'skate3.exe'
if (-not (Test-Path $exe)) { throw "No skate3.exe in $Build." }
$dest = Join-Path $Folder '_recomp'
$bin = Join-Path $dest 'bin'
New-Item -ItemType Directory -Force $bin | Out-Null
Get-ChildItem $bin -File | Remove-Item -Force
Copy-Item $exe $bin
Get-ChildItem $Build -File -Filter 'rexruntime*.dll' | Copy-Item -Destination $bin
# Which build this is: the recomp checkout's commit (the build folder sits in <checkout>\out\build\<preset>).
$repo = Split-Path (Split-Path (Split-Path $Build))
$commit = ''
if (Test-Path (Join-Path $repo '.git')) {
    $ErrorActionPreference = 'Continue'
    $commit = (& git.exe -C $repo log -1 --format='%h %s' 2>$null)
    $ErrorActionPreference = 'Stop'
}
"$((Get-Item $exe).LastWriteTime.ToString('s')) $commit" | Set-Content -Encoding ASCII (Join-Path $bin 'build.txt')
Write-Host "Recomp build staged in $bin ($((Get-ChildItem $bin -File).Count) files)."

if ($WithGame) {
    if (-not $Game -or -not (Test-Path (Join-Path $Game 'game\default.xex'))) { throw '-WithGame needs -Game <recomp game folder with game\default.xex>.' }
    # The game data only: no executable, logs or launcher files.
    & robocopy.exe $Game (Join-Path $dest 'game') /E /NFL /NDL /NJH /NP /R:2 /W:5 /XD logs .local /XF skate3.exe rexruntime*.dll LastPlayed.txt quiver-launch.log | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE)." }
    Write-Host "Game folder staged in $dest\game; play-recomp.ps1 -GetGame on the second PC copies it and removes this copy."
}

if ($WithSave) {
    if (Get-Process skate3 -ErrorAction SilentlyContinue) { throw 'Close the recomp first: its save is written while it runs.' }
    $save = Join-Path $env:APPDATA 'skate3'
    if (-not (Test-Path $save)) { throw "No recomp save in $save." }
    $out = Join-Path $dest 'save\skate3'
    & robocopy.exe $save $out /MIR /NFL /NDL /NJH /NP /R:2 /W:5 /XD cache '_backup_*' | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE)." }
    Write-Host "Save staged in $out; play-recomp.ps1 -GetSave on the second PC copies it (after backing up its own)."
}
