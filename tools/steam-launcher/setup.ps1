# First-time setup for the Steam launcher: checks what a fresh Windows PC needs to build and play versions of this
# repository and installs what is missing with winget. Windows PowerShell 5.1 compatible.
#   setup.ps1             the check: one line per tool (id, name, ok 1/0, detail, required 1/0); exit 0 = all required there
#   setup.ps1 -Install    installs what is missing: ONE UAC prompt for the machine-wide tools (installed one after
#                         another: two installers at once broke the Rust install on the laptop), then Rust for this
#                         user (never elevated: it installs into the user's profile)
# launcher.ps1 dot-sources this file for Missing-Tools. Sources for each check: docs\hails-additions\second-pc-testing.md.
param([switch]$Install, [switch]$Elevated, [string]$Ids = '', [string]$Log = '')

$WingetFlags = @('--exact', '--silent', '--accept-package-agreements', '--accept-source-agreements')

function Refresh-Path {
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User') + ";$env:USERPROFILE\.cargo\bin"
}
function Has([string]$Cmd) { [bool](Get-Command $Cmd -ErrorAction SilentlyContinue) }
function VS-Path([string]$Requires) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    $p = & $vswhere -latest -products * -requires $Requires -property installationPath 2>$null | Select-Object -First 1
    if ($p) { return $p }; return $null
}

# id, name, required, machine-wide (needs the elevated step), winget id + extra args, check
$Tools = @(
    @{ Id = 'git';    Name = 'Git';                                   Req = $true;  Machine = $true;  Winget = 'Git.Git'
       Check = { if (Has 'git.exe') { (& git --version) } } }
    @{ Id = 'vs';     Name = 'C++ build tools (MSVC, Windows SDK, CMake)'; Req = $true; Machine = $true; Winget = 'Microsoft.VisualStudio.2022.BuildTools'
       Extra = @('--override', '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.VC.CMake.Project --add Microsoft.VisualStudio.Component.Windows11SDK.26100')
       Check = {
           $vs = VS-Path 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64'
           $rc = @(Get-ChildItem (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin') -Filter rc.exe -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.DirectoryName -like '*\x64' }).Count -gt 0
           $cmake = (Has 'cmake.exe') -or ($vs -and (Test-Path (Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe')))
           if ($vs -and $rc -and $cmake) { "Build Tools at $vs" }
       } }
    @{ Id = 'llvm';   Name = 'LLVM (llvm-readobj for the build)';    Req = $true;  Machine = $true;  Winget = 'LLVM.LLVM'
       Check = { $p = Join-Path $env:ProgramFiles 'LLVM\bin\llvm-readobj.exe'; if (Test-Path $p) { $p } } }
    @{ Id = 'python'; Name = 'Python 3.13 (converts the game files)'; Req = $true;  Machine = $true;  Winget = 'Python.Python.3.13'; Extra = @('--scope', 'machine')
       Check = { if (Has 'py.exe') { $v = & py -3.13 --version 2>$null; if ($LASTEXITCODE -eq 0) { $v } } } }
    @{ Id = 'rust';   Name = 'Rust (rustup, stable)';                 Req = $true;  Machine = $false; Winget = 'Rustlang.Rustup'
       Check = { if (Has 'cargo.exe') { $t = & rustup show active-toolchain 2>$null; if ($LASTEXITCODE -eq 0 -and $t) { "$t" } } } }
    @{ Id = 'gh';     Name = 'GitHub CLI (optional: sending logs to GitHub)'; Req = $false; Machine = $true; Winget = 'GitHub.cli'
       Check = { if ((Has 'gh.exe') -or (Test-Path (Join-Path $env:ProgramFiles 'GitHub CLI\gh.exe'))) { 'installed' } } }
    @{ Id = 'vulkan'; Name = 'Vulkan (comes with the graphics driver)'; Req = $true; Machine = $false; Winget = ''
       Check = { if (Test-Path (Join-Path $env:WINDIR 'System32\vulkan-1.dll')) { 'vulkan-1.dll present' } } }
)

# Every version the launcher lists comes from a prebuilt release (launcher.ps1 sets SKATE_LAUNCHER_RELEASES=1): the
# build tools are not needed, only Git (the launcher's own updates) and Vulkan.
if ($env:SKATE_LAUNCHER_RELEASES -eq '1') { foreach ($t in $Tools) { if ($t.Id -in @('vs', 'llvm', 'python', 'rust')) { $t.Req = $false } } }
function Tool-State($t) { try { $r = & $t.Check; if ($r) { return "$r".Trim() } } catch {}; return $null }
# The required tools still missing, by name ('' = none). launcher.ps1 greys out the build entries while this says so.
function Missing-Tools {
    Refresh-Path
    return (@($Tools | Where-Object { $_.Req -and -not (Tool-State $_) } | ForEach-Object { $_.Name }) -join ', ')
}

function Winget-Install($t) {
    Write-Host "  Installing $($t.Name) ($($t.Winget))..."
    $a = @('install', '--id', $t.Winget) + $WingetFlags
    if ($t.Extra) { $a += $t.Extra }
    # Quiet: winget's progress bars would fill the window (and the elevated step's log).
    & winget @a | Out-Null
    Write-Host "  $($t.Name): winget exit code $LASTEXITCODE"
}

if ($MyInvocation.InvocationName -eq '.') { return }

if ($Elevated) {
    # The one elevated step: the machine-wide installs, one after another, logged for the launcher to show.
    Start-Transcript -Path $Log -Force | Out-Null
    foreach ($id in ($Ids -split ',' | Where-Object { $_ })) { $t = $Tools | Where-Object { $_.Id -eq $id }; if ($t) { Winget-Install $t } }
    # Long paths: cargo's deep target folders under a long checkout path.
    try { Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled -Value 1 -Type DWord; Write-Host '  Long paths: on' } catch { Write-Host "  Long paths: $($_.Exception.Message)" }
    Stop-Transcript | Out-Null
    exit 0
}

Refresh-Path
$missing = @()
foreach ($t in $Tools) {
    $s = Tool-State $t
    if (-not $s) { $missing += $t }
    "{0}`t{1}`t{2}`t{3}`t{4}" -f $t.Id, $t.Name, $(if ($s) { 1 } else { 0 }), $(if ($s) { $s } else { 'missing' }), $(if ($t.Req) { 1 } else { 0 })
}
if (-not $Install) { exit $(if (@($missing | Where-Object { $_.Req }).Count -eq 0) { 0 } else { 1 }) }
if ($env:SKATE_LAUNCHER_RELEASES -eq '1') { $missing = @($missing | Where-Object { $_.Req }) }

Write-Host ''
if ($missing.Count -eq 0) { Write-Host '  Everything is installed.'; exit 0 }
if (-not (Has 'winget.exe')) { Write-Host '  winget is missing: install "App Installer" from the Microsoft Store, then run the setup again.'; exit 1 }
$machine = @($missing | Where-Object { $_.Machine -and $_.Winget })
if ($machine.Count -gt 0) {
    Write-Host ("  Installing " + (($machine | ForEach-Object { $_.Name }) -join ', ') + '.')
    Write-Host '  Windows asks once for permission (UAC). The C++ build tools take 10 minutes or more.'
    $log = Join-Path $env:TEMP ('skate-launcher-setup-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
    $argList = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Elevated -Ids $(($machine | ForEach-Object { $_.Id }) -join ',') -Log `"$log`""
    try { Start-Process powershell.exe -Verb RunAs -ArgumentList $argList -Wait -WindowStyle Hidden }
    catch { Write-Host '  The permission prompt was declined: nothing was installed.'; exit 1 }
    if (Test-Path $log) { Get-Content $log | Where-Object { $_ -match '^\s+\S' -and $_ -notmatch '^\*' } | ForEach-Object { Write-Host $_ }; Remove-Item $log -ErrorAction SilentlyContinue }
}
if ($missing | Where-Object { $_.Id -eq 'rust' }) {
    Winget-Install ($Tools | Where-Object { $_.Id -eq 'rust' })
    Refresh-Path
    if (Has 'rustup.exe') { & rustup default stable 2>&1 | ForEach-Object { Write-Host "  $_" } }
}
if ($missing | Where-Object { $_.Id -eq 'vulkan' }) { Write-Host '  Vulkan is missing: install or update the graphics driver (NVIDIA, AMD or Intel).' }
Refresh-Path
Write-Host ''
$left = @($Tools | Where-Object { $_.Req -and -not (Tool-State $_) })
if ($left.Count -eq 0) { Write-Host $(if ($env:SKATE_LAUNCHER_RELEASES -eq '1') { '  All required tools are installed. Next: Play.' } else { '  All required tools are installed. Next: convert the game files (tools\setup-dev-assets.ps1 -Disc <your Skate 3 ISO>).' }); exit 0 }
Write-Host ("  Still missing: " + (($left | ForEach-Object { $_.Name }) -join ', '))
exit 1
