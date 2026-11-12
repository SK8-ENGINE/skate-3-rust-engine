# Where addresses of the TU3 image are referenced: code/data references plus raw word matches (vtable slots).
# usage: find-refs.ps1 [-Out file] <address> [address ...]
# Project: -ProjectDir (default $env:GHIDRA_PROJECT_DIR), -ProjectName skate3-tu3, -Program default.xexp; needs GHIDRA_INSTALL_DIR.
[CmdletBinding(PositionalBinding = $false)]
param([string]$Out = (Join-Path $PSScriptRoot "..\..\..\..\.local\research\ghidra\refs.txt"),
      [string]$ProjectDir = $env:GHIDRA_PROJECT_DIR,
      [string]$ProjectName = "skate3-tu3",
      [string]$Program = "default.xexp",
      [Parameter(ValueFromRemainingArguments = $true)][string[]]$Addresses)
if (-not $Addresses) { Write-Error "usage: find-refs.ps1 [-Out file] <address> [address ...]"; exit 2 }
if (-not $ProjectDir) { Write-Error "set GHIDRA_PROJECT_DIR (or pass -ProjectDir) to the folder holding your Ghidra project"; exit 2 }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$Out = (Resolve-Path (Split-Path $Out)).Path + "\" + (Split-Path $Out -Leaf)
$tools = $PSScriptRoot
$log = "$Out.log"
& (Join-Path $tools "ghidra_headless.ps1") -MaxMem 8G $ProjectDir $ProjectName -process $Program -noanalysis `
    -scriptPath (Join-Path $tools "scripts") -postScript FindRefs.java $Out @Addresses > $log
if (-not (Test-Path $Out)) { Get-Content $log | Select-String "ERROR|Exception" | Select-Object -First 10; exit 1 }
Get-Content $Out
