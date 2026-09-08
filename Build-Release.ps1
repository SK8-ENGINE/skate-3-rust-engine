param([string]$TargetDirectory = (Join-Path $PSScriptRoot 'target'))
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    $packagePython = Join-Path $PSScriptRoot 'target/package-venv/Scripts/python.exe'
    if (-not (Test-Path -LiteralPath $packagePython)) {
        & python -m venv target/package-venv
        if ($LASTEXITCODE -ne 0) { throw 'Could not create packaging environment' }
    }
    & $packagePython -m pip install -r tools/requirements-setup.txt
    if ($LASTEXITCODE -ne 0) { throw 'Could not install packaging dependencies' }
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-ffff'
    $stage = Join-Path $PSScriptRoot "target/release-packages/$stamp/skate3rust-windows-x64"
    $symbols = Join-Path $PSScriptRoot "target/release-packages/$stamp/symbols"
    New-Item -ItemType Directory -Path "$stage/support",$symbols -Force | Out-Null
    # Link this invocation directly into private staging; never copy a generic cache EXE.
    & cargo rustc --release --locked --target x86_64-pc-windows-msvc --target-dir $TargetDirectory -p skate-game --bin skate3rust --no-default-features -- -C extra-filename= -o "$stage/skate3rust.exe" -C "link-arg=/PDB:$symbols/skate3rust.pdb"
    if ($LASTEXITCODE -ne 0) { throw 'Release compilation failed' }
    New-Item -ItemType Directory -Path target/native -Force | Out-Null
    & rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort -C target-feature=+crt-static tools/asset_pipeline/refpack_native.rs -o target/native/refpack.dll
    if ($LASTEXITCODE -ne 0) { throw 'Native converter compilation failed' }
    if (-not (Test-Path -LiteralPath "$stage/skate3rust.exe") -or -not (Test-Path -LiteralPath "$symbols/skate3rust.pdb")) { throw 'Fresh executable or matching symbols missing.' }
    # Preserve the exact PE and PDB pair privately; symbols are not in the player ZIP.
    Copy-Item -LiteralPath "$stage/skate3rust.exe" -Destination $symbols
    @{
        revision = (& git rev-parse HEAD).Trim()
        source_status = @(& git status --porcelain)
        executable_sha256 = (Get-FileHash -LiteralPath "$stage/skate3rust.exe" -Algorithm SHA256).Hash
        pdb_sha256 = (Get-FileHash -LiteralPath "$symbols/skate3rust.pdb" -Algorithm SHA256).Hash
        compiler = (& rustc --version).Trim()
    } | ConvertTo-Json | Set-Content -LiteralPath "$symbols/build.json" -Encoding UTF8
    $sourceStage = Join-Path $stage '../setup-source'
    $toolsRoot = Join-Path $PSScriptRoot 'tools'
    foreach ($source in Get-ChildItem -LiteralPath $toolsRoot -File -Recurse) {
        if ($source.FullName -match '[\\/]__pycache__[\\/]') { continue }
        if ($source.Extension -notin '.py','.json','.txt','.md','.toml' -and $source.Name -ne 'LICENSE') { continue }
        $relative = [IO.Path]::GetRelativePath($toolsRoot, $source.FullName)
        $portableName = $relative.Replace('\','/')
        if ($portableName -match '(^|/)blender[^/]*(/|$)' -or
            $portableName -in @('asset_pipeline/build_map.py','asset_pipeline/finish_character.py',
                'add_onboard_ik_targets.py','apply_default_skater_materials.py','export_bevy_glb.py')) { continue }
        $destination = Join-Path "$sourceStage/tools" $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath $source.FullName -Destination $destination
    }
    & $packagePython -m PyInstaller --noconfirm --clean --onefile --name skate3setup `
        --icon "$PSScriptRoot/docs/images/skating-crab.ico" --paths $PSScriptRoot `
        --hidden-import numpy --hidden-import PIL.Image --hidden-import tkinter `
        --add-binary "$PSScriptRoot/target/native/refpack.dll;tools/asset_pipeline" `
        --exclude-module bpy --exclude-module mathutils `
        --copy-metadata numpy --copy-metadata Pillow --copy-metadata PyInstaller `
        --add-data "$sourceStage/tools;tools" --add-data "$PSScriptRoot/docs/images/skating-crab.ico;docs/images" `
        --distpath "$stage/support" --workpath target/setup-build/work --specpath target/setup-build tools/setup.py
    if ($LASTEXITCODE -ne 0) { throw 'Setup packaging failed' }
    Copy-Item -LiteralPath README.md,THIRD_PARTY_NOTICES.md -Destination $stage
    New-Item -ItemType Directory -Path "$stage/docs/images" -Force | Out-Null
    Copy-Item -LiteralPath docs/images/skating-crab.png -Destination "$stage/docs/images/skating-crab.png"
    Copy-Item -LiteralPath docs/installation.md -Destination "$stage/docs/installation.md"
    Copy-Item -LiteralPath docs/retail-renderer.md -Destination "$stage/docs/retail-renderer.md"
    Copy-Item -LiteralPath docs/crash-reports.md -Destination "$stage/docs/crash-reports.md"
    New-Item -ItemType Directory -Path "$stage/licenses" -Force | Out-Null
    Copy-Item -LiteralPath tools/vendor/utt/LICENSE -Destination "$stage/licenses/UTT.txt"
    Copy-Item -LiteralPath tools/vendor/university/LICENSE-PROJECT.md -Destination "$stage/licenses/CustomEngineLayer.txt"
    Copy-Item -LiteralPath vendor/bevy_pbr/LICENSE-MIT -Destination "$stage/licenses/Bevy-MIT.txt"
    Copy-Item -LiteralPath vendor/bevy_pbr/LICENSE-APACHE -Destination "$stage/licenses/Bevy-APACHE.txt"
    $pythonBase = (& $packagePython -c 'import sys; print(sys.base_prefix)').Trim()
    Copy-Item -LiteralPath "$pythonBase/LICENSE.txt" -Destination "$stage/licenses/Python.txt"
    foreach ($license in Get-ChildItem -LiteralPath "$pythonBase/tcl" -Filter license.terms -Recurse -ErrorAction SilentlyContinue) {
        Copy-Item -LiteralPath $license.FullName -Destination "$stage/licenses/$($license.Directory.Name).txt"
    }
    $zip = Join-Path $PSScriptRoot 'target/skate3rust-windows-x64.zip'
    Compress-Archive -LiteralPath $stage -DestinationPath $zip -Force
    (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLower() + '  skate3rust-windows-x64.zip' |
        Set-Content -LiteralPath "$zip.sha256" -Encoding ascii
    Write-Host "Release package: $zip"
} finally { Pop-Location }
