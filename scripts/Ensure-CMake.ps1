# Locate CMake for the statically linked SDL3 build (sdl3-sys build-from-source).
# CI runners have cmake on PATH; local installs often only have Visual Studio's copy.
$ErrorActionPreference = 'Stop'
if ($env:CMAKE -and (Test-Path -LiteralPath $env:CMAKE)) { return }
if (Get-Command cmake -ErrorAction SilentlyContinue) { return }

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path -LiteralPath $vswhere) {
    $cmake = & $vswhere -latest -products * -find 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe' |
        Select-Object -First 1
    if ($cmake -and (Test-Path -LiteralPath $cmake)) {
        $env:CMAKE = $cmake
        return
    }
}
throw 'CMake is required to build SDL3. Install CMake, or the "C++ CMake tools for Windows" Visual Studio component.'
