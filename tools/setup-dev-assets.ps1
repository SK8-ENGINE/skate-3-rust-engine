# Source checkout: convert the game files from your Skate 3 disc into data\ and point assets\ at them (the same setup
# the release build runs on first start, without its window). Run from anywhere, after building bin\skate3rust.exe:
#   powershell -ExecutionPolicy Bypass -File tools\setup-dev-assets.ps1 -Disc "C:\path\to\Skate 3.iso"
# -Disc takes the .iso or an extracted disc's default.xex. Re-run it after converter changes to refresh what changed.
# Needs Python 3.13 (py -3.13); numpy and Pillow are installed into .local\venv-setup on the first run.
param(
    [Parameter(Mandatory)][string]$Disc
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$exe = Join-Path $root 'bin\skate3rust.exe'
if (-not (Test-Path $exe)) { throw "Build the game first (bin\skate3rust.exe is missing): see README.md, Build." }
$Disc = (Resolve-Path $Disc).ProviderPath
if (-not (Get-Command py -ErrorAction SilentlyContinue)) { throw 'Python launcher (py) not found: winget install --id Python.Python.3.13 -e, then open a new terminal.' }

# Python environment with the converter's packages (pyinstaller is only for building the release setup exe).
$venv = Join-Path $root '.local\venv-setup'
$python = Join-Path $venv 'Scripts\python.exe'
if (-not (Test-Path $python)) {
    Write-Host "Creating $venv (Python 3.13)..."
    & py -3.13 -m venv $venv
    if ($LASTEXITCODE -ne 0) { throw 'py -3.13 failed: install Python 3.13 (winget install --id Python.Python.3.13 -e).' }
}
$packages = Get-Content (Join-Path $root 'tools\requirements-setup.txt') | Where-Object { $_ -and $_ -notmatch '^pyinstaller' }
& $python -m pip install --disable-pip-version-check -q @packages
if ($LASTEXITCODE -ne 0) { throw 'pip could not install the setup packages.' }

# The conversion (several minutes; refreshes only what changed when data\ already has an install).
$refresh = Test-Path (Join-Path $root 'data\installation.json')
$script = Join-Path $env:TEMP "setup-dev-assets-$PID.py"
@"
import sys
from pathlib import Path
root = Path(sys.argv[1])
sys.path.insert(0, str(root))
from tools.asset_pipeline.customiser_setup import install
install(Path(sys.argv[2]), root / 'data', root / 'bin' / 'skate3rust.exe', lambda *a, **k: print(*a, flush=True), refresh=sys.argv[3] == '1')
"@ | Set-Content -Encoding ASCII $script
try {
    Write-Host "Converting the game files from $Disc (several minutes)..."
    & $python -u $script $root $Disc $(if ($refresh) { '1' } else { '0' })
    if ($LASTEXITCODE -ne 0) { throw 'The conversion failed (see the lines above).' }
} finally { Remove-Item $script -ErrorAction SilentlyContinue }

# assets\ -> the new installation (setup publishes a new data\installations\<id> each time).
$dir = (Get-Content (Join-Path $root 'data\installation.json') -Raw | ConvertFrom-Json).directory
$target = Join-Path $root "data\$dir\assets"
$link = Join-Path $root 'assets'
$old = Get-Item $link -Force -ErrorAction SilentlyContinue
if ($old) {
    if ($old.LinkType -ne 'Junction') { throw "$link exists and is not a junction; move it away first." }
    $old.Delete()
}
New-Item -ItemType Junction -Path $link -Target $target | Out-Null
Write-Host "assets -> $target"

# The game logs to stderr; Windows PowerShell 5.1 turns those lines into errors, fatal under Stop.
$ErrorActionPreference = 'Continue'
$check = & $exe --assets $link --check-assets 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($code -eq 0 -and $check -contains 'SKATE_ASSETS_READY') { Write-Host 'SKATE_ASSETS_READY: the game files are ready.' }
else { $check | Select-Object -Last 10; throw 'The game files did not pass the check (see above).' }
