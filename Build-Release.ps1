param()
$ErrorActionPreference = 'Stop'
throw 'Release packaging is paused until the Blender-free ISO converter is ready.'
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
    & cargo build --release --locked --target x86_64-pc-windows-msvc -p skate-game --no-default-features
    if ($LASTEXITCODE -ne 0) { throw 'Release compilation failed' }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $stage = Join-Path $PSScriptRoot "target/release-packages/$stamp/skate3rust-windows-x64"
    New-Item -ItemType Directory -Path "$stage/support" -Force | Out-Null
    Copy-Item -LiteralPath target/x86_64-pc-windows-msvc/release/skate3rust.exe -Destination "$stage/skate3rust.exe"
    $sourceStage = Join-Path $stage '../setup-source'
    $toolsRoot = Join-Path $PSScriptRoot 'tools'
    foreach ($source in Get-ChildItem -LiteralPath $toolsRoot -File -Recurse) {
        if ($source.FullName -match '[\\/]__pycache__[\\/]') { continue }
        if ($source.Extension -notin '.py','.json','.txt','.md','.toml' -and $source.Name -ne 'LICENSE') { continue }
        $relative = [IO.Path]::GetRelativePath($toolsRoot, $source.FullName)
        $destination = Join-Path "$sourceStage/tools" $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath $source.FullName -Destination $destination
    }
    & $packagePython -m PyInstaller --noconfirm --clean --onefile --name skate3setup `
        --icon "$PSScriptRoot/docs/images/skating-crab.ico" --paths $PSScriptRoot `
        --hidden-import numpy --hidden-import PIL.Image --hidden-import tkinter `
        --copy-metadata numpy --copy-metadata Pillow --copy-metadata PyInstaller `
        --add-data "$sourceStage/tools;tools" --add-data "$PSScriptRoot/docs/images/skating-crab.ico;docs/images" `
        --distpath "$stage/support" --workpath target/setup-build/work --specpath target/setup-build tools/setup.py
    if ($LASTEXITCODE -ne 0) { throw 'Setup packaging failed' }
    Copy-Item -LiteralPath README.md,THIRD_PARTY_NOTICES.md -Destination $stage
    New-Item -ItemType Directory -Path "$stage/docs/images" -Force | Out-Null
    Copy-Item -LiteralPath docs/images/skating-crab.png -Destination "$stage/docs/images/skating-crab.png"
    Copy-Item -LiteralPath docs/installation.md -Destination "$stage/docs/installation.md"
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
