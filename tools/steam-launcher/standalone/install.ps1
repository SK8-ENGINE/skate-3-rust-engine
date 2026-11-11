# Standalone Skate launcher, Windows (INSTALL.bat runs this): downloads the Skate launcher for Windows from the fork's
# rolling release, checks it against the release's SHA-256, runs the first-time setup (setup.ps1: what this PC needs,
# installed with one UAC prompt) and starts the launcher. Run it again to update or repair. Doc: README.md
# "Launcher-only download (standalone)".
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$base = 'https://github.com/Hailey-Ross/rusty-trucks/releases/download/hails-additions'
$asset = 'skate-launcher-windows-x64.exe'
$part = Join-Path $here "$asset.part"
try {
    Write-Host 'Downloading the Skate launcher for Windows...'
    & curl.exe -fsSL --max-time 900 -o $part "$base/$asset"
    if ($LASTEXITCODE -ne 0) { throw "The download failed ($base/$asset). Check the internet connection and run INSTALL.bat again." }
    $sum = & curl.exe -fsSL --max-time 60 "$base/$asset.sha256"
    if ($LASTEXITCODE -ne 0) { throw 'The checksum could not be downloaded. Run INSTALL.bat again.' }
    $want = (($sum -join ' ').Trim() -split '\s+')[0].ToLowerInvariant()
    $got = (Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($want -ne $got) { throw 'The download is damaged (its checksum does not match). Run INSTALL.bat again.' }
    $exe = Join-Path $here 'SkateLauncher.exe'
    Move-Item -LiteralPath $part -Destination $exe -Force
    # The new (Rust) launcher; Tools > Switch back to the classic launcher builds SkateLauncher.cs instead.
    [IO.File]::WriteAllText((Join-Path $here 'launcher-flavour.txt'), "rust`r`n")
} catch {
    if (Test-Path -LiteralPath $part) { Remove-Item -LiteralPath $part -Force }
    Write-Host $_.Exception.Message -ForegroundColor Red
    Read-Host 'Press Enter to close'
    exit 1
}
Write-Host 'Checking what this PC needs...'
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here 'setup.ps1') | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host 'Installing what is missing (Windows asks once for permission)...'
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here 'setup.ps1') -Install
}
Write-Host 'Starting the Skate launcher.'
Start-Process -FilePath (Join-Path $here 'SkateLauncher.exe') -WorkingDirectory $here
