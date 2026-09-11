# Locate rc.exe for skate-game icon embedding. Debug and release builds share build.rs.
$ErrorActionPreference = 'Stop'
if ($env:RC -and (Test-Path -LiteralPath $env:RC)) { return }

if (-not ${env:ProgramFiles(x86)}) {
    $fallback = 'C:\Program Files (x86)'
    if (Test-Path -LiteralPath $fallback) {
        ${env:ProgramFiles(x86)} = $fallback
    }
}
if (-not ${env:ProgramFiles(x86)}) {
    throw 'ProgramFiles(x86) is not set and C:\Program Files (x86) was not found. Run from a normal Command Prompt or install the Windows SDK.'
}

$sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
if (-not (Test-Path -LiteralPath $sdkBin)) {
    throw "Windows 10 SDK not found at $sdkBin. Install it with Visual Studio (Desktop development with C++) or the standalone Windows SDK."
}

$rc = Get-ChildItem -LiteralPath $sdkBin -Directory |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'x64\rc.exe' } |
    Where-Object { Test-Path -LiteralPath $_ } |
    Select-Object -First 1

if (-not $rc) {
    throw "Windows SDK resource compiler (rc.exe) not found under $sdkBin. Install the Windows 10 SDK build tools or set RC to rc.exe."
}

$env:RC = $rc
