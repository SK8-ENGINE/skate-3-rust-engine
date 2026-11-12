# Names the decoded TU3 functions in your Ghidra project from a names TSV (skill ghidra, "Function names").
# Opens the project like decompile.ps1 (no -readOnly), so the names and plate comments are saved.
# Usage: apply-names.ps1 [-Tsv <file>] [-Log <file>] [-ProjectDir <dir>]   (defaults: the repo's
# tools\ghidra\skate3-tu3\skate3-tu3-functions.tsv, <Tsv>.log, $env:GHIDRA_PROJECT_DIR); needs GHIDRA_INSTALL_DIR.
[CmdletBinding(PositionalBinding = $false)]
param([string]$Tsv = (Join-Path $PSScriptRoot "..\..\..\..\tools\ghidra\skate3-tu3\skate3-tu3-functions.tsv"),
      [string]$Log = "",
      [string]$ProjectDir = $env:GHIDRA_PROJECT_DIR,
      [string]$ProjectName = "skate3-tu3",
      [string]$Program = "default.xexp")
if (-not $ProjectDir) { Write-Error "set GHIDRA_PROJECT_DIR (or pass -ProjectDir) to the folder holding your Ghidra project"; exit 2 }
if (-not (Test-Path $Tsv)) { Write-Error "no TSV at $Tsv"; exit 2 }
$Tsv = (Resolve-Path $Tsv).Path
if (-not $Log) { $Log = "$Tsv.log" }
$tools = $PSScriptRoot
& (Join-Path $tools "ghidra_headless.ps1") -MaxMem 8G $ProjectDir $ProjectName -process $Program -noanalysis `
    -scriptPath (Join-Path $tools "scripts") -postScript ApplyNames.java $Tsv $Log > "$Log.headless"
if (-not (Test-Path $Log)) { Get-Content "$Log.headless" | Select-String "ERROR|Exception" | Select-Object -First 10; exit 1 }
Remove-Item "$Log.headless"
Get-Content $Log | Select-String "^SKIP|^applied"
