param([switch]$StageOnly, [string]$DependencyTargetDirectory = (Join-Path $PSScriptRoot 'target'))
$ErrorActionPreference = 'Stop'
$privateDirectory = Join-Path $PSScriptRoot '.local/session-marker'
$buildDirectory = Join-Path $privateDirectory 'build'
$artifact = Join-Path $buildDirectory 'skate3rust-session-marker.exe'
$destination = Join-Path $privateDirectory 'skate3-session-marker.exe'
Push-Location $PSScriptRoot
$previousFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
try {
    New-Item -ItemType Directory -Force -Path $buildDirectory | Out-Null
    if (-not $StageOnly) {
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
        # Dependencies may use a shared cache. The linker writes this task's
        # artifact directly, never staging the cache's generic skate3rust.exe.
        & cargo rustc --release --locked --target x86_64-pc-windows-msvc `
            --target-dir $DependencyTargetDirectory -p skate-game --no-default-features `
            --bin skate3rust -- -C extra-filename=-session-marker `
            -o (Join-Path $buildDirectory 'skate3rust.exe')
        if ($LASTEXITCODE -ne 0) { throw 'Session-marker build failed.' }
    }
    $bytes = [System.IO.File]::ReadAllBytes($artifact)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    foreach ($required in @('--start-paused', 'session_marker/effect.wgsl', 'Session marker return rejected')) {
        if (-not $text.Contains($required)) { throw "Wrong session-marker artifact: missing $required" }
    }
    Copy-Item -LiteralPath $artifact -Destination $destination -Force
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
    if ($hash -ne (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash) {
        throw 'Staged executable hash mismatch.'
    }
    @{ executable = $destination; sha256 = $hash; source = $PSScriptRoot;
       revision = (& git rev-parse HEAD); staticChecks = 'CLI, marker shader, return path' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $privateDirectory 'build-manifest.json') -Encoding UTF8
    Write-Host "Ready for manual launch: $destination"
    Write-Host "SHA256: $hash"
} finally {
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $previousFlags
    Pop-Location
}
