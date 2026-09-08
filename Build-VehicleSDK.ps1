param([switch]$StageOnly, [string]$DependencyTargetDirectory = (Join-Path $PSScriptRoot 'target'))
$ErrorActionPreference = 'Stop'
$privateDirectory = Join-Path $PSScriptRoot '.local/vehicle-sdk'
$buildDirectory = Join-Path $privateDirectory 'build'
$artifact = Join-Path $buildDirectory 'skate3rust-vehicle-sdk.exe'
$destination = Join-Path $privateDirectory 'skate3-vehicle-bails.exe'
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
        foreach ($crate in @('skate-core', 'skate-data', 'skate-mods', 'skate-vehicles')) {
            (Get-Item -LiteralPath (Join-Path $PSScriptRoot "crates/$crate/src/lib.rs")).LastWriteTime = Get-Date
        }
        & cargo rustc --release --locked --target x86_64-pc-windows-msvc `
            --target-dir $DependencyTargetDirectory -p skate-game --no-default-features `
            --bin skate3rust -- -C extra-filename=-vehicle-sdk `
            -o (Join-Path $buildDirectory 'skate3rust.exe')
        if ($LASTEXITCODE -ne 0) { throw 'Lua SDK build failed.' }
    }
    $bytes = [System.IO.File]::ReadAllBytes($artifact)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    foreach ($required in @('--start-paused', 'Wheel or drag scrollbar', 'Drag // to resize', 'hold_fakie', 'Vehicle key already spawned', 'ZIP must contain mod.json at its root', 'Native trainer controls are already owned by another mod')) {
        if (-not $text.Contains($required)) { throw "Wrong vehicle-sdk artifact: missing $required" }
    }
    Copy-Item -LiteralPath $artifact -Destination $destination -Force
    # Build ZIPs from the editable SDK examples. The project launcher reads root/mods.
    $modRoot = Join-Path $privateDirectory 'mods'
    New-Item -ItemType Directory -Force -Path $modRoot | Out-Null
    foreach ($name in @('native-trainer','mario-kart')) {
        & python (Join-Path $PSScriptRoot 'tools/package_mod.py') (Join-Path $PSScriptRoot "sdk/examples/$name") (Join-Path $PSScriptRoot "mods/$name.zip") --target-dir (Join-Path $PSScriptRoot '.local/vehicle-tests')
        if ($LASTEXITCODE -ne 0) { throw "Failed to package $name" }
        $old = [System.IO.Path]::GetFullPath((Join-Path $modRoot $name))
        if (Test-Path -LiteralPath $old -PathType Container) {
            $archive = [System.IO.Path]::GetFullPath((Join-Path $modRoot ('.legacy-' + $name + '-' + [guid]::NewGuid().ToString('N'))))
            $bound = [System.IO.Path]::GetFullPath($modRoot) + [System.IO.Path]::DirectorySeparatorChar
            if (-not $old.StartsWith($bound) -or -not $archive.StartsWith($bound)) { throw 'Invalid mod migration path' }
            Move-Item -LiteralPath $old -Destination $archive
        }
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot "mods/$name.zip") -Destination (Join-Path $modRoot "$name.zip") -Force
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'mods/README.md') -Destination (Join-Path $modRoot 'README.md') -Force
    New-Item -ItemType Directory -Force -Path (Join-Path $privateDirectory 'docs'), (Join-Path $privateDirectory 'sdk') | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs/vehicle-sdk.md') -Destination (Join-Path $privateDirectory 'docs/vehicle-sdk.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs/mixamo-vehicle-workflow.md') -Destination (Join-Path $privateDirectory 'docs/mixamo-vehicle-workflow.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sdk/AGENTS.md') -Destination (Join-Path $privateDirectory 'sdk/AGENTS.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs/mod-packages.md') -Destination (Join-Path $privateDirectory 'docs/mod-packages.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs/lua-modding.md') -Destination (Join-Path $privateDirectory 'docs/lua-modding.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sdk/skate.lua') -Destination (Join-Path $privateDirectory 'sdk/skate.lua') -Force
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
