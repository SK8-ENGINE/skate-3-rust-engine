# Builds everything needed to run the local multiplayer harness, and stages it
# into a single `dist/` folder so a plain double-click works (no cargo run, no
# missing-DLL startup failures).
#
# Output layout:
#   dist/
#     skated.exe            authoritative session server (Go, static)
#     skate3rust.exe        game client (Rust release)
#     bevy_dylib.dll        only when built with dynamic linking
#     assets/               prepared assets the client needs (junction)
#     multiplayer.bat       launcher (see dist/multiplayer.bat)
#     Launch-Multiplayer.ps1 configurable multi-instance launcher
#
# Usage (from skate-3-rust-engine/):
#     powershell -File scripts/build-multiplayer.ps1           # static (default)
#     powershell -File scripts/build-multiplayer.ps1 -Dynamic  # dynamic linking
#
# Static (no dev-dynamic) avoids requiring Bevy DLLs beside the executable.
#
# Requires: cargo, go, and protoc (or set $env:PROTOC).
param(
    [switch]$Dynamic
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root 'dist'
$repo = Split-Path -Parent $root

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }

# protoc is not always on PATH in a fresh shell (winget install).
if (-not $env:PROTOC) {
    $candidate = 'C:\Users\goust\AppData\Local\Microsoft\WinGet\Packages\Google.Protobuf_Microsoft.Winget.Source_8wekyb3d8bbwe\bin\protoc.exe'
    if (Test-Path $candidate) { $env:PROTOC = $candidate }
}

Step 'Building the Rust game client (release)'
Push-Location $root
try {
    if ($Dynamic) {
        cargo build --release -p skate-game --bin skate3rust
    } else {
        cargo build --release -p skate-game --bin skate3rust --no-default-features
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}

Step 'Building the Go session server'
$serverGo = Join-Path $repo 'server-go'
if (-not (Test-Path $serverGo)) { throw "server-go not found at $serverGo" }
Push-Location $serverGo
try {
    go build -o bin/skated.exe ./cmd/skated
    if ($LASTEXITCODE -ne 0) { throw "go build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}

Step 'Staging into dist/'
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Copy-Item (Join-Path $root 'target\release\skate3rust.exe') (Join-Path $dist 'skate3rust.exe') -Force
Copy-Item (Join-Path $serverGo 'bin\skated.exe') (Join-Path $dist 'skated.exe') -Force

# Dynamic linking needs bevy_dylib.dll beside the exe; static does not.
$dylib = Join-Path $dist 'bevy_dylib.dll'
if ($Dynamic) {
    Copy-Item (Join-Path $root 'target\release\bevy_dylib.dll') $dylib -Force
} else {
    Remove-Item $dylib -Force -ErrorAction SilentlyContinue
}

# Assets: link the client's assets directory next to the exe. A junction keeps
# the large asset tree out of dist/ while still resolving `SKATE3_ASSETS`.
$assetsSource = Join-Path $root 'assets'
$assetsLink = Join-Path $dist 'assets'
if (Test-Path $assetsLink) {
    # Remove an old junction/link (not a real directory copy).
    $item = Get-Item $assetsLink -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        cmd /c rmdir "$assetsLink" | Out-Null
    } else {
        throw "Refusing to replace an existing real assets directory: $assetsLink"
    }
}
if (Test-Path $assetsSource) {
    cmd /c mklink /J "$assetsLink" "$assetsSource" | Out-Null
} else {
    Write-Warning "assets/ not found; the client will not start without SKATE3_ASSETS"
}

Step 'Copying the launcher'
Copy-Item (Join-Path $PSScriptRoot 'multiplayer.bat') (Join-Path $dist 'multiplayer.bat') -Force
Copy-Item (Join-Path $PSScriptRoot 'Launch-Multiplayer.ps1') (Join-Path $dist 'Launch-Multiplayer.ps1') -Force

Write-Host ''
Write-Host "Done. Run: $dist\multiplayer.bat" -ForegroundColor Green
