param([string]$Map)
$ErrorActionPreference = 'Stop'
if (-not $Map) {
    $maps = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'maps') -Filter '*.skate' -File -Recurse | Sort-Object FullName)
    if ($maps.Count -eq 0) { throw 'Put a .skate file in maps/private, or drag one onto PLAY-MAP.bat.' }
    for ($i = 0; $i -lt $maps.Count; $i++) { Write-Host "$($i + 1). $($maps[$i].Name)" }
    $choice = Read-Host 'Map number (Enter cancels)'
    if (-not $choice) { exit 0 }
    $selected = 0
    if (-not [int]::TryParse($choice, [ref]$selected) -or $selected -lt 1 -or $selected -gt $maps.Count) { throw 'Invalid map number.' }
    $Map = $maps[$selected - 1].FullName
}
& (Join-Path $PSScriptRoot 'Launch.ps1') -Map $Map
