param([switch]$StageOnly, [string]$DependencyTargetDirectory = (Join-Path $PSScriptRoot 'target'))
$ErrorActionPreference = 'Stop'
$privateDirectory = Join-Path $PSScriptRoot '.local/mod-windows'
$buildDirectory = Join-Path $privateDirectory 'build'
$artifact = Join-Path $buildDirectory 'skate3rust-mod-windows.exe'
$destination = Join-Path $privateDirectory 'skate3-mod-windows.exe'
Push-Location $PSScriptRoot
$previousFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
try {
    New-Item -ItemType Directory -Force -Path $buildDirectory | Out-Null
    if (-not $StageOnly) {
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
        # Dependencies may use a shared cache. The linker writes this task's
        # artifact directly, never staging the cache's generic skate3rust.exe.
        # Shared target fingerprints can refer to another worktree's local crates.
        # Refresh only our local source timestamps; never clean the shared cache.
        foreach ($crate in @('skate-core', 'skate-data', 'skate-mods')) {
            (Get-Item -LiteralPath (Join-Path $PSScriptRoot "crates/$crate/src/lib.rs")).LastWriteTime = Get-Date
        }
        & cargo rustc --release --locked --target x86_64-pc-windows-msvc `
            --target-dir $DependencyTargetDirectory -p skate-game --no-default-features `
            --bin skate3rust -- -C extra-filename=-mod-windows `
            -o (Join-Path $buildDirectory 'skate3rust.exe')
        if ($LASTEXITCODE -ne 0) { throw 'Lua SDK build failed.' }
    }
    $bytes = [System.IO.File]::ReadAllBytes($artifact)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    foreach ($required in @('--start-paused', 'Drag title to move', 'Native trainer controls are already owned by another mod')) {
        if (-not $text.Contains($required)) { throw "Wrong mod-windows artifact: missing $required" }
    }
    Copy-Item -LiteralPath $artifact -Destination $destination -Force
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
    if ($hash -ne (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash) {
        throw 'Staged executable hash mismatch.'
    }
    @{ executable = $destination; sha256 = $hash; source = $PSScriptRoot;
       revision = (& git rev-parse HEAD); staticChecks = 'CLI, Mods UI, Lua quota diagnostics' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $privateDirectory 'build-manifest.json') -Encoding UTF8
    Write-Host "Ready for manual launch: $destination"
    Write-Host "SHA256: $hash"
} finally {
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $previousFlags
    Pop-Location
}
