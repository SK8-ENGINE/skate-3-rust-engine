# Pseudocode of TU3 functions from your Ghidra project (skill ghidra): one JVM start per call, so pass every address
# you need at once. Reference only: never copy the output into the repo.
# Usage: decompile.ps1 [-Out <file>] [-ProjectDir <dir>] [-ProjectName <name>] [-Program <name>] <address> [address ...]
#        (82E0E580, 0x82E0E580 or sub_82E0E580)
# Defaults: -ProjectDir $env:GHIDRA_PROJECT_DIR (<ghidra-project-dir>, required), -ProjectName skate3-tu3,
#           -Program default.xexp, -Out <repo>\.local\research\ghidra\decompile.txt (gitignored), also printed.
# Needs GHIDRA_INSTALL_DIR (and JAVA_HOME) for ghidra_headless.ps1.
[CmdletBinding(PositionalBinding = $false)]
param([string]$Out = (Join-Path $PSScriptRoot "..\..\..\..\.local\research\ghidra\decompile.txt"),
      [string]$ProjectDir = $env:GHIDRA_PROJECT_DIR,
      [string]$ProjectName = "skate3-tu3",
      [string]$Program = "default.xexp",
      [Parameter(ValueFromRemainingArguments = $true)][string[]]$Addresses)
if (-not $Addresses) { Write-Error "usage: decompile.ps1 [-Out file] <address> [address ...]"; exit 2 }
if (-not $ProjectDir) { Write-Error "set GHIDRA_PROJECT_DIR (or pass -ProjectDir) to the folder holding your Ghidra project"; exit 2 }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$Out = (Resolve-Path (Split-Path $Out)).Path + "\" + (Split-Path $Out -Leaf)
$tools = $PSScriptRoot
$log = "$Out.log"
& (Join-Path $tools "ghidra_headless.ps1") -MaxMem 8G $ProjectDir $ProjectName -process $Program -noanalysis `
    -scriptPath (Join-Path $tools "scripts") -postScript DecompileAt.java $Out @Addresses > $log
if (-not (Test-Path $Out)) { Get-Content $log | Select-String "ERROR|Exception" | Select-Object -First 10; exit 1 }
Get-Content $Out
