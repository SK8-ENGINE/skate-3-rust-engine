# Where exact C strings sit in the TU3 image and which functions reference them (registered names, enum labels).
# usage: find-strings.ps1 [-Out file] <string> [string ...]
# Project: -ProjectDir (default $env:GHIDRA_PROJECT_DIR), -ProjectName skate3-tu3, -Program default.xexp; needs GHIDRA_INSTALL_DIR.
# References come from Ghidra's analysis: a string built with lis/addi that the analyzer did not resolve has none.
[CmdletBinding(PositionalBinding = $false)]
param([string]$Out = (Join-Path $PSScriptRoot "..\..\..\..\.local\research\ghidra\strings.txt"),
      [string]$ProjectDir = $env:GHIDRA_PROJECT_DIR,
      [string]$ProjectName = "skate3-tu3",
      [string]$Program = "default.xexp",
      [Parameter(ValueFromRemainingArguments = $true)][string[]]$Strings)
if (-not $Strings) { Write-Error "usage: find-strings.ps1 [-Out file] <string> [string ...]"; exit 2 }
if (-not $ProjectDir) { Write-Error "set GHIDRA_PROJECT_DIR (or pass -ProjectDir) to the folder holding your Ghidra project"; exit 2 }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$Out = (Resolve-Path (Split-Path $Out)).Path + "\" + (Split-Path $Out -Leaf)
$tools = $PSScriptRoot
$log = "$Out.log"
& (Join-Path $tools "ghidra_headless.ps1") -MaxMem 8G $ProjectDir $ProjectName -process $Program -noanalysis `
    -scriptPath (Join-Path $tools "scripts") -postScript FindStrings.java $Out @Strings > $log
if (-not (Test-Path $Out)) { Get-Content $log | Select-String "ERROR|Exception" | Select-Object -First 10; exit 1 }
Get-Content $Out
