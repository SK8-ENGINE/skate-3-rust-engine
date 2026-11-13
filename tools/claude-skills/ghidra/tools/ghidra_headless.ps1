# Ghidra headless (AnalyzeHeadless) without analyzeHeadless.bat: the batch layer breaks option names with spaces and
# parentheses (the Xenon360 loader's "Base XEX path (required for XEXP)"). Same Java call as support\launch.bat:
# the launch.properties VM arguments from LaunchSupport, -Xmx, Utility.jar, ghidra.Ghidra <class> <args>.
# Usage: ghidra_headless.ps1 [-MaxMem 8G] <AnalyzeHeadless arguments...>
# Needs: GHIDRA_INSTALL_DIR = <ghidra-install> (Ghidra 12.1.x with the Xenon360 extension, skill ghidra);
#        JAVA_HOME = a JDK 21 or newer (otherwise java.exe from PATH).
param([string]$MaxMem = "8G", [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest)
# Java writes warnings to stderr: keep going (Ghidra's own exit code decides).
$ErrorActionPreference = "Continue"
$install = $env:GHIDRA_INSTALL_DIR
if (-not $install -or -not (Test-Path (Join-Path $install "support\LaunchSupport.jar"))) {
    Write-Error "set GHIDRA_INSTALL_DIR to your Ghidra install folder (<ghidra-install>)"; exit 2
}
$java = if ($env:JAVA_HOME) { Join-Path $env:JAVA_HOME "bin\java.exe" } else { "java.exe" }
$vmargs = & $java -cp (Join-Path $install "support\LaunchSupport.jar") LaunchSupport $install -vmargs |
    ForEach-Object { $_.Trim() } | Where-Object { $_ }
$cmd = @($vmargs) + @("-Xmx$MaxMem", "-Duser.home=$env:USERPROFILE", "-cp", (Join-Path $install "Ghidra\Framework\Utility\lib\Utility.jar"),
    "ghidra.Ghidra", "ghidra.app.util.headless.AnalyzeHeadless") + $Rest
& $java @cmd 2>&1 | ForEach-Object { "$_" }
exit $LASTEXITCODE
