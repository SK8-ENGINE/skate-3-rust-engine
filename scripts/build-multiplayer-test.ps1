param([string]$OutputDirectory = 'bin/multiplayer')
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $workspace
$env:CARGO_TARGET_DIR = 'C:/Users/Daddy/Documents/skate3-imported/target'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
& cargo build --release --target x86_64-pc-windows-msvc -p skate-game --bin skate3-multiplayer --no-default-features --locked
if ($LASTEXITCODE -ne 0) { throw 'Multiplayer build failed.' }
& cargo build --release --target x86_64-pc-windows-msvc -p skate-steam-relay --locked
if ($LASTEXITCODE -ne 0) { throw 'Steam relay build failed.' }
$output = Join-Path $workspace $OutputDirectory
$relay = Join-Path $output 'steam-relay'
New-Item -ItemType Directory -Path $relay -Force | Out-Null
$release = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/release'
Copy-Item -LiteralPath (Join-Path $release 'skate3-multiplayer.exe') -Destination $output
Copy-Item -LiteralPath (Join-Path $release 'skate-steam-relay.exe') -Destination $relay
$metadata = (& cargo metadata --format-version 1 --locked) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Steam redistributable.' }
# Extract only the JSON string we need. Windows PowerShell's JSON object parser
# rejects unrelated dependency feature keys that differ only in letter case.
$sdk = [regex]::Match($metadata, '"name":"steamworks-sys","version":"0\.13\.0".*?"manifest_path":("(?:[^"\\]|\\.)*")')
if (-not $sdk.Success) { throw 'Steamworks SDK package was not found in Cargo metadata.' }
$manifest = ConvertFrom-Json -InputObject $sdk.Groups[1].Value
$dll = Join-Path (Split-Path -Parent $manifest) 'lib/steam/redistributable_bin/win64/steam_api64.dll'
Copy-Item -LiteralPath $dll -Destination $relay
Write-Host "Staged multiplayer build: $output"
Write-Host 'Build only; no game or Steam process was launched.'
