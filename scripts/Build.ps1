param(
    [switch]$StageOnly,
    [switch]$WithRelay,
    [string]$TargetDirectory = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target')
)
# Fast local iteration: Bevy dynamic_linking (default features) + stable target/.
# Do NOT pass --no-default-features here — that static-links Bevy and takes many minutes.
$ProjectRoot = Split-Path $PSScriptRoot -Parent
$ErrorActionPreference = 'Stop'
$sw = [Diagnostics.Stopwatch]::StartNew()
Push-Location $ProjectRoot
try {
    if (-not $StageOnly) {
        $packages = @('-p', 'skate-game')
        if ($WithRelay) { $packages += @('-p', 'skate-steam-relay') }
        & cargo build @packages --bin skate3rust --target-dir $TargetDirectory
        if ($LASTEXITCODE -ne 0) { throw 'Build failed; see the compiler output above.' }
    }
    $debugDirectory = Join-Path $TargetDirectory 'debug'
    $executable = Join-Path $debugDirectory 'skate3rust.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw "Missing executable: $executable" }
    $readobj = Join-Path $env:ProgramFiles 'LLVM/bin/llvm-readobj.exe'
    if (-not (Test-Path -LiteralPath $readobj)) { throw 'LLVM llvm-readobj is required to stage exact runtime DLL dependencies.' }
    $rustLibraries = (& rustc --print target-libdir).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not locate Rust runtime libraries.' }
    $binDirectory = Join-Path $ProjectRoot 'bin'
    New-Item -ItemType Directory -Path $binDirectory -Force | Out-Null
    $queue = [System.Collections.Generic.Queue[string]]::new()
    $queue.Enqueue($executable)
    $seen = @{}
    $staged = @()
    while ($queue.Count -gt 0) {
        $source = $queue.Dequeue()
        $name = Split-Path -Leaf $source
        if ($seen.ContainsKey($name)) { continue }
        $seen[$name] = $true
        $destination = Join-Path $binDirectory $name
        Copy-Item -LiteralPath $source -Destination $destination -Force
        $staged += @{name = $name; sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash}
        # Keep development backtraces useful after staging the EXE and Bevy DLLs.
        $pdb = [IO.Path]::ChangeExtension($source, '.pdb')
        if (Test-Path -LiteralPath $pdb) {
            $symbolDestination = Join-Path $binDirectory (Split-Path -Leaf $pdb)
            Copy-Item -LiteralPath $pdb -Destination $symbolDestination -Force
            $staged += @{name = (Split-Path -Leaf $pdb); sha256 = (Get-FileHash -LiteralPath $symbolDestination -Algorithm SHA256).Hash}
        }
        $imports = & $readobj --coff-imports $source
        if ($LASTEXITCODE -ne 0) { throw "Could not inspect DLL imports: $source" }
        foreach ($line in $imports) {
            if ($line -match '^\s+Name: (.+\.dll)$') {
                $dependency = $Matches[1]
                $found = $false
                foreach ($directory in @($debugDirectory, (Join-Path $debugDirectory 'deps'), $rustLibraries)) {
                    $candidate = Join-Path $directory $dependency
                    if (Test-Path -LiteralPath $candidate) {
                        $queue.Enqueue($candidate)
                        $found = $true
                        break
                    }
                }
                if (-not $found -and $dependency -notmatch '^(api-ms-|ext-ms-)' -and
                    -not (Test-Path -LiteralPath (Join-Path $env:WINDIR "System32/$dependency"))) {
                    throw "Runtime dependency not found: $dependency"
                }
            }
        }
    }
    if ($WithRelay -or (Test-Path -LiteralPath (Join-Path $binDirectory 'steam-relay/skate-steam-relay.exe'))) {
        if ($WithRelay) {
            & (Join-Path $PSScriptRoot 'Stage-SteamRelay.ps1') -TargetDirectory $TargetDirectory -BinDirectory $binDirectory
        }
        foreach ($name in @('steam-relay/skate-steam-relay.exe', 'steam-relay/steam_api64.dll')) {
            $path = Join-Path $binDirectory $name
            if (Test-Path -LiteralPath $path) {
                $staged += @{name = $name; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
            }
        }
    }
    $staged | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $binDirectory 'manifest.json') -Encoding UTF8
    Write-Host ("Ready: {0}/skate3rust.exe ({1:n1}s)" -f $binDirectory, $sw.Elapsed.TotalSeconds)
} finally { Pop-Location }
