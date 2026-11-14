# Second PC: play with a session log (like PLAY-WITH-LOG.bat), then send every log and crash report not sent yet to
# the main PC's shared folder (\\<main PC>\SkateSessions\<this PC's name>\). A log that cannot be sent (main PC off)
# stays in logs\ and goes with the next send. First run asks for the main PC's name and the share account's password
# (saved in Windows Credential Manager, never in a file).
# Release ZIP: put it with PLAY-AND-SEND.bat next to skate3rust.exe. Source checkout: run it where it is
# (tools\second-pc); it then works on the checkout root (logs\, bin\skate3rust.exe, send-sessions.txt there).
# Guide: docs/hails-additions/second-pc-testing.md
param(
    # Only send what is pending, without playing.
    [switch]$SendOnly,
    # Ask for the main PC and password again.
    [switch]$Setup,
    # Check the GitHub fallback: send one small test file as a pull request (no game, share not used).
    [switch]$GitHubTest,
    # Upload part size for the GitHub fallback (tests use small parts).
    [int]$PartMB = 40
)
$ErrorActionPreference = 'Stop'
# The game folder: next to the script (release ZIP), else the checkout this script is part of (tools\second-pc).
$Root = $PSScriptRoot
$checkout = Split-Path (Split-Path $PSScriptRoot)
if (-not (Test-Path (Join-Path $Root 'skate3rust.exe')) -and (Test-Path (Join-Path $checkout 'Cargo.toml'))) { $Root = $checkout }
Set-Location $Root
$configFile = Join-Path $Root 'send-sessions.txt'
$sentFile = Join-Path $Root 'logs\sent.txt'
$crashDir = Join-Path $env:LOCALAPPDATA 'Skate3RustEngine\CrashReports'
New-Item -ItemType Directory -Force -Path (Join-Path $Root 'logs') | Out-Null

# Open the share with the login saved by setup and return the server that answered (the name, else the IP; on
# Wi-Fi the name lookup failed now and then with "System error 67"), or $null. Until an SMB session exists, a plain
# Test-Path can say "does not exist" on a share that works.
function Connect-Share($c) {
    foreach ($server in @($c.main_pc, $c.main_ip) | Where-Object { $_ }) {
        $unc = "\\$server\$($c.share)"
        # Windows PowerShell 5.1 turns a native program's stderr into errors, fatal under Stop: run net with Continue.
        $ErrorActionPreference = 'Continue'
        & net.exe use $unc /persistent:no 2>&1 | Out-Null
        if (Test-Path $unc) { return $server }
    }
    return $null
}

# The GitHub CLI, also when this process started before it was installed (Steam keeps an old PATH until sign-out).
function Get-Gh {
    $cmd = Get-Command gh -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $exe = Join-Path $env:ProgramFiles 'GitHub CLI\gh.exe'
    if (Test-Path $exe) { return $exe }
    return $null
}

function Read-Config {
    if (-not (Test-Path $configFile)) { return $null }
    $c = @{}
    foreach ($line in Get-Content $configFile) { if ($line -match '^\s*(\w+)\s*=\s*(.*?)\s*$') { $c[$Matches[1]] = $Matches[2] } }
    $c
}

function Invoke-Setup {
    Write-Host 'One-time setup: where should session logs go?'
    $mainPc = Read-Host "Main PC's name (Settings > System > About on the main PC, e.g. MY-DESKTOP)"
    $ip = ''
    try { $ip = ([Net.Dns]::GetHostAddresses($mainPc) | Where-Object { $_.AddressFamily -eq 'InterNetwork' } | Select-Object -First 1).IPAddressToString } catch {}
    $answer = Read-Host "Main PC's IP address, used when the name does not answer [$ip]"
    if ($answer) { $ip = $answer }
    $share = Read-Host 'Share name [SkateSessions]'
    if (-not $share) { $share = 'SkateSessions' }
    $account = Read-Host 'Account on the main PC [skatelogs]'
    if (-not $account) { $account = 'skatelogs' }
    $user = "$mainPc\$account"
    $password = Read-Host "Password of $user (saved in Windows Credential Manager, never in a file)" -AsSecureString
    $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($password))
    foreach ($server in @($mainPc, $ip) | Where-Object { $_ }) {
        & cmdkey.exe "/add:$server" "/user:$user" "/pass:$plain" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "cmdkey could not save the login for $server." }
    }
    $plain = $null
    $github = ''
    $answer = Read-Host 'When the main PC cannot be reached, send the logs as a pull request on GitHub instead? (needs the GitHub CLI, signed in; the repo is PUBLIC) [y/N]'
    if ($answer -match '^[yY]') {
        $github = Read-Host 'Repository [Hailey-Ross/rusty-trucks]'
        if (-not $github) { $github = 'Hailey-Ross/rusty-trucks' }
        # A missing gh must only warn: with ErrorActionPreference Stop it would end setup before the config is saved.
        $signedIn = $false
        $gh = Get-Gh
        if ($gh) { $ErrorActionPreference = 'Continue'; & $gh auth status 2>&1 | Out-Null; $signedIn = $LASTEXITCODE -eq 0; $ErrorActionPreference = 'Stop' }
        if (-not $signedIn) { Write-Host 'The GitHub CLI is missing or not signed in: winget install GitHub.cli, then gh auth login.' }
    }
    "main_pc=$mainPc`nmain_ip=$ip`nshare=$share`naccount=$user`ngithub_repo=$github`ngithub_base=hails-additions" |
        Set-Content -Encoding ASCII $configFile
    $server = Connect-Share (Read-Config)
    if ($server) { Write-Host "OK: \\$server\$share is reachable." }
    else { Write-Host "Could not reach \\$mainPc\$share or \\$ip\$share yet (main PC off, wrong password?). Logs stay here until it works." }
}

# True when nobody holds the file open for writing (open for reading, sharing only reads).
function Test-Readable([string]$Path) {
    try { $s = [IO.File]::Open($Path, 'Open', 'Read', 'Read'); $s.Dispose(); return $true } catch { return $false }
}

function Send-Pending {
    $c = Read-Config
    if (-not $c) { Write-Host 'Not set up: run with -Setup.'; return }
    $target = "\\$($c.main_pc)\$($c.share)\$env:COMPUTERNAME"   # updated to the server that answers
    $sent = @{}
    if (Test-Path $sentFile) { foreach ($n in Get-Content $sentFile) { $sent[$n] = $true } }
    # This checkout and every Steam launcher version (.local\versions\<id>, plus the one the launcher names in
    # SKATE_SEND_LOGS_DIR): a version's sessions write to its own logs\ (laptop 2026-10-18: only a crash report went).
    $roots = @($Root) + @(Get-ChildItem (Join-Path $Root '.local\versions') -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    if ($env:SKATE_SEND_LOGS_DIR) { $roots += $env:SKATE_SEND_LOGS_DIR }
    $roots = @($roots | Where-Object { Test-Path $_ } | ForEach-Object { (Resolve-Path $_).Path } | Sort-Object -Unique)
    # Game logs, plus the launcher's build logs (<version>\logs\launcher-build-*.log) so a failed build reaches main.
    $files = @($roots | ForEach-Object { Get-ChildItem -Path (Join-Path $_ 'logs\*') -File -Include 'game-*.log', 'launcher-build-*.log' -ErrorAction SilentlyContinue })
    if (Test-Path $crashDir) { $files += @(Get-ChildItem $crashDir -File) }
    # Session folders: logs\trace-all-<time> (the Steam launcher's rust-traceall), bin\logs\trace-all-<unix seconds>
    # (the engine's default next to the exe) and logs\recomp-<preset>-<time> (play-recomp.ps1). While a game runs, a
    # folder written to in the last 30 s may still be in use; once no game runs (the send right after a session) every
    # folder goes.
    $gameRunning = [bool](Get-Process skate3rust, skate3 -ErrorAction SilentlyContinue)
    foreach ($dir in @($roots | ForEach-Object { (Join-Path $_ 'logs'), (Join-Path $_ 'bin\logs') })) {
        $files += @(Get-ChildItem $dir -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'trace-all-*' -or $_.Name -like 'recomp-*' } | Where-Object {
            if (-not $gameRunning) { return $true }
            $newest = Get-ChildItem $_.FullName -Recurse -File | Sort-Object LastWriteTime -Descending | Select-Object -First 1
            -not $newest -or $newest.LastWriteTime -lt (Get-Date).AddSeconds(-30)
        })
    }
    $pending = @($files | Where-Object { -not $sent[$_.FullName] })
    # A file another process still writes (a second launcher's open build log, laptop 2026-10-19) cannot be copied or
    # zipped: it waits for the next send instead of failing the whole send.
    $busy = @($pending | Where-Object { -not $_.PSIsContainer -and -not (Test-Readable $_.FullName) })
    foreach ($f in $busy) { Write-Host "Skipped $($f.Name): still open in another program; it goes with the next send." }
    $pending = @($pending | Where-Object { $busy -notcontains $_ })
    if ($pending.Count -eq 0) { Write-Host 'Nothing new to send.'; return }
    try {
        $server = Connect-Share $c
        if (-not $server) { throw "neither $($c.main_pc) nor $($c.main_ip) answered" }
        $target = "\\$server\$($c.share)\$env:COMPUTERNAME"
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        foreach ($f in $pending) {
            $dest = if ($f.PSIsContainer) { $target } elseif ($f.DirectoryName -eq $crashDir) { Join-Path $target 'CrashReports' } else { $target }
            New-Item -ItemType Directory -Force -Path $dest | Out-Null
            Copy-Item -LiteralPath $f.FullName -Destination $dest -Force -Recurse
            Add-Content -Path $sentFile -Value $f.FullName
            Write-Host "Sent $($f.Name)"
        }
        Write-Host "All sent to $target"
    } catch {
        $script:ShareError = "share: $($_.Exception.Message)"
        Write-Host "Could not send to $target ($($_.Exception.Message))."
        $left = @($pending | Where-Object { -not (Select-String -Path $sentFile -SimpleMatch $_.FullName -Quiet -ErrorAction SilentlyContinue) })
        # A recomp session with the captured game sound (audio.f32) never goes to the public repository: it waits
        # for the share.
        $left = @($left | Where-Object { -not ($_.PSIsContainer -and $_.Name -like 'recomp-*' -and (Test-Path (Join-Path $_.FullName 'audio.f32'))) })
        if ($c.github_repo -and $left.Count -gt 0) { Send-ToGitHub $c $left }
        else { Write-Host 'The logs stay in logs\ and go with the next send.' }
    }
}

# Fallback: zip the files, put the zip on a new branch session-logs/<PC>-<time> through the GitHub API (no clone
# needed) and open a pull request; the main PC fetches it (fetch-github-sessions.ps1) and closes it.
function Send-ToGitHub($c, $files) {
    $repo = $c.github_repo
    $base = if ($c.github_base) { $c.github_base } else { 'hails-additions' }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $branch = "session-logs/$($env:COMPUTERNAME.ToLower())-$stamp"
    $gh = Get-Gh
    if (-not $gh) { Write-Host 'GitHub fallback: the GitHub CLI is not installed (winget install --id GitHub.cli -e). The logs stay in logs\ and go with the next send.'; return }
    # gh writes progress to stderr; under Stop Windows PowerShell 5.1 would treat that as a fatal error.
    $ErrorActionPreference = 'Continue' 
    $work = Join-Path $env:TEMP "skate-session-$stamp"
    try {
        New-Item -ItemType Directory -Force -Path $work | Out-Null
        $zip = Join-Path $work "sessions-$($env:COMPUTERNAME)-$stamp.zip"
        Compress-Archive -LiteralPath ($files | ForEach-Object FullName) -DestinationPath $zip -CompressionLevel Optimal
        # GitHub's contents API takes files up to about 100 MB (and the request is base64): upload the zip in 40 MB
        # parts <zip>.001, .002, ...; fetch-github-sessions.ps1 joins them again.
        $part = $PartMB * 1MB
        $bytes = [IO.File]::ReadAllBytes($zip)
        $count = [math]::Max(1, [math]::Ceiling($bytes.Length / $part))
        $sha = & $gh api "repos/$repo/git/ref/heads/$base" --jq '.object.sha'
        if ($LASTEXITCODE -ne 0 -or -not $sha) { throw "cannot read branch $base of $repo (gh signed in?)" }
        & $gh api -X POST "repos/$repo/git/refs" -f "ref=refs/heads/$branch" -f "sha=$sha" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "cannot create branch $branch" }
        $body = Join-Path $work 'body.json'
        for ($i = 0; $i -lt $count; $i++) {
            $len = [math]::Min($part, $bytes.Length - $i * $part)
            $name = (Split-Path $zip -Leaf) + $(if ($count -gt 1) { '.{0:D3}' -f ($i + 1) } else { '' })
            @{ message = "Session logs from $env:COMPUTERNAME ($stamp) $name"; branch = $branch
               content = [Convert]::ToBase64String($bytes, $i * $part, $len) } | ConvertTo-Json | Set-Content -Encoding ASCII $body
            & $gh api -X PUT "repos/$repo/contents/session-logs/$name" --input $body | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "cannot upload $name" }
        }
        $names = ($files | ForEach-Object { "- $($_.Name)" }) -join "`n"
        $url = & $gh pr create --repo $repo --base $base --head $branch --title "Session logs from $env:COMPUTERNAME ($stamp)" `
            --body "Session logs sent from $env:COMPUTERNAME because the main PC could not be reached. Not for merging: the main PC fetches the zip and closes this.`n`n$names"
        if ($LASTEXITCODE -ne 0) { throw 'cannot open the pull request' }
        foreach ($f in $files) { Add-Content -Path $sentFile -Value $f.FullName }
        Write-Host "Sent $($files.Count) file(s) to GitHub instead: $url"
    } catch {
        Write-Host "GitHub fallback failed too ($($_.Exception.Message)). The logs stay in logs\ and go with the next send.$(if ($script:ShareError) { " (First the $($script:ShareError))" })"
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
}

if ($GitHubTest) {
    $c = Read-Config
    if (-not $c -or -not $c.github_repo) { throw 'The GitHub fallback is not set up: run with -Setup and answer y.' }
    # A small trace-all-like folder (a text file and a table), so folders and split parts get checked too.
    $test = Join-Path $env:TEMP "trace-all-githubtest-$env:COMPUTERNAME"
    New-Item -ItemType Directory -Force -Path $test | Out-Null
    "GitHub fallback test from $env:COMPUTERNAME at $(Get-Date -Format s)" | Set-Content (Join-Path $test 'note.txt')
    $rng = New-Object Random 7
    $buf = New-Object byte[] (3MB); $rng.NextBytes($buf); [IO.File]::WriteAllBytes((Join-Path $test 'noise.bin'), $buf)
    $sentFile = Join-Path $env:TEMP 'github-fallback-test-sent.txt'
    Send-ToGitHub $c @(Get-Item $test)
    Remove-Item -Recurse $test, $sentFile -ErrorAction SilentlyContinue
    return
}
if ($Setup -or -not (Read-Config)) { Invoke-Setup; if ($Setup) { return } }
if (-not $SendOnly) {
    $exe = Join-Path $Root 'skate3rust.exe'
    $play = Join-Path $Root 'PLAY.bat'
    if (Test-Path $exe) {
        # Release ZIP: log like PLAY-WITH-LOG.bat.
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $log = Join-Path $Root "logs\game-$stamp.log"
        $errLog = Join-Path $Root "logs\game-$stamp.stderr.log"
        Write-Host "Playing with logging: $errLog"
        Start-Process -FilePath $exe -RedirectStandardOutput $log -RedirectStandardError $errLog -Wait
    } elseif (Test-Path $play) {
        # Source checkout: PLAY.bat starts bin\skate3rust.exe with the checkout's assets and logs to logs\ itself.
        Write-Host "Playing with $play"
        & cmd.exe /c "`"$play`""
    } else { throw "No skate3rust.exe or PLAY.bat in $Root." }
    Write-Host 'Session finished.'
}
Send-Pending
