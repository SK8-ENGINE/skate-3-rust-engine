# Share sessions as a GitHub issue: docs/hails-additions/second-pc-testing.md#share-sessions-via-github
param(
    [string]$Root = '',
    [string]$Settings = '',
    [string]$Repo = '',
    [string]$Notes = '',
    [int]$PartMB = 25,
    [switch]$Consent,
    [switch]$Revoke,
    [switch]$Status,
    [switch]$NoOpen
)
$ErrorActionPreference = 'Stop'
if (-not $Root) {
    $Root = if (Test-Path (Join-Path $PSScriptRoot 'skate3rust.exe')) { $PSScriptRoot } else { (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path }
}
$settingsFile = if ($Settings) { $Settings } else { Join-Path $Root 'share-sessions.txt' }
$sharedFile = Join-Path (Split-Path $settingsFile) 'shared-sessions.txt'
$ConsentVersion = 1

function Read-Settings {
    $s = @{}
    if (Test-Path $settingsFile) {
        foreach ($line in Get-Content $settingsFile) { if ($line -match '^\s*([^=#]+?)\s*=\s*(.*)$') { $s[$Matches[1]] = $Matches[2].Trim() } }
    }
    return $s
}

function Write-Settings($s) {
    ($s.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join "`n" | Set-Content -Path $settingsFile -Encoding ASCII
}

function Get-RepoSlug {
    $s = Read-Settings
    if ($Repo) { return $Repo }
    if ($s['repo']) { return $s['repo'] }
    $url = ''
    $ErrorActionPreference = 'Continue'
    $url = (& git.exe -C $Root remote get-url origin 2>$null | Select-Object -First 1)
    $global:LASTEXITCODE = 0
    $ErrorActionPreference = 'Stop'
    if ($url -match 'github\.com[:/]+([^/]+)/([^/]+?)(\.git)?/?$') { return "$($Matches[1])/$($Matches[2])" }
    $baked = Join-Path $Root 'share-repo.txt'
    if (Test-Path $baked) { return (Get-Content $baked -TotalCount 1).Trim() }
    return ''
}

function Get-Build {
    $ErrorActionPreference = 'Continue'
    $commit = (& git.exe -C $Root rev-parse --short HEAD 2>$null | Select-Object -First 1)
    $branch = (& git.exe -C $Root rev-parse --abbrev-ref HEAD 2>$null | Select-Object -First 1)
    $global:LASTEXITCODE = 0
    $ErrorActionPreference = 'Stop'
    if (-not $commit) { $commit = 'unknown' }
    if (-not $branch -or $branch -eq 'HEAD') { $branch = 'detached' }
    return "$branch $commit"
}

function Get-Pending {
    $shared = @{}
    if (Test-Path $sharedFile) { foreach ($n in Get-Content $sharedFile) { $shared[$n] = $true } }
    $files = @(Get-ChildItem -Path (Join-Path $Root 'logs\*') -File -Include 'game-*.log', 'launcher-build-*.log' -ErrorAction SilentlyContinue)
    foreach ($dir in (Join-Path $Root 'logs'), (Join-Path $Root 'bin\logs')) {
        $files += @(Get-ChildItem $dir -Directory -ErrorAction SilentlyContinue | Where-Object {
            ($_.Name -like 'trace-all-*' -or $_.Name -like 'recomp-*') -and -not (Test-Path (Join-Path $_.FullName 'audio.f32'))
        })
    }
    return @($files | Where-Object { -not $shared[$_.FullName] })
}

function Protect-Text([string]$Text) {
    foreach ($pair in @(@($env:USERPROFILE, '<home>'), @($env:USERNAME, '<user>'), @($env:COMPUTERNAME, '<pc>'))) {
        if ($pair[0]) { $Text = $Text -replace [regex]::Escape($pair[0]), $pair[1] }
    }
    return $Text
}

function Copy-Scrubbed($Item, [string]$Dest) {
    $text = '.log', '.txt', '.tsv', '.csv', '.json', '.md', '.ini', '.toml'
    $sources = if ($Item.PSIsContainer) { Get-ChildItem $Item.FullName -Recurse -File } else { @($Item) }
    $base = if ($Item.PSIsContainer) { $Item.Parent.FullName } else { $Item.DirectoryName }
    foreach ($f in $sources) {
        $out = Join-Path $Dest (Protect-Text $f.FullName.Substring($base.Length).TrimStart('\'))
        New-Item -ItemType Directory -Force -Path (Split-Path $out) | Out-Null
        if ($text -contains $f.Extension.ToLower()) {
            [IO.File]::WriteAllText($out, (Protect-Text ([IO.File]::ReadAllText($f.FullName))))
        } else {
            Copy-Item -LiteralPath $f.FullName -Destination $out -Force
        }
    }
}

function Split-Zip([string]$Zip, [int]$MB) {
    $limit = $MB * 1MB
    $len = (Get-Item $Zip).Length
    if ($len -le $limit) { return @($Zip) }
    $bytes = [IO.File]::ReadAllBytes($Zip)
    $parts = @()
    for ($i = 0; $i * $limit -lt $len; $i++) {
        $n = [Math]::Min($limit, $len - $i * $limit)
        $part = '{0}.{1:D3}' -f $Zip, ($i + 1)
        $chunk = New-Object byte[] $n
        [Array]::Copy($bytes, $i * $limit, $chunk, 0, $n)
        [IO.File]::WriteAllBytes($part, $chunk)
        $parts += $part
    }
    Remove-Item $Zip -Force
    return $parts
}

function Get-IssueUrl([string]$Slug, [string]$Title, [string]$Body) {
    $e = { param($v) [Uri]::EscapeDataString($v) }
    return "https://github.com/$Slug/issues/new?template=session-report.md&labels=session-report&title=$(& $e $Title)&body=$(& $e $Body)"
}

$s = Read-Settings
if ($Revoke) { $s['consent'] = '0'; Write-Settings $s; Write-Host 'Sharing via GitHub is off.'; return }
if ($Consent) { $s['consent'] = "$ConsentVersion"; $s['mode'] = 'github'; Write-Settings $s; Write-Host "Sharing via GitHub is on ($(Get-RepoSlug))."; return }
$on = $s['mode'] -eq 'github' -and $s['consent'] -eq "$ConsentVersion"
$slug = Get-RepoSlug
if ($Status) { "share`t$(if (-not $on) { 'off' } elseif (-not $slug) { 'no repo' } else { "on $slug" })"; return }
if (-not $on) { Write-Host 'Sharing via GitHub is off (no consent). Nothing was packed.'; return }
if (-not $slug) { Write-Host 'No GitHub repository known for this build. Nothing was packed.'; return }

$pending = Get-Pending
if ($pending.Count -eq 0) { Write-Host 'Nothing new to share.'; return }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$outDir = Join-Path $Root 'logs\shared'
$stage = Join-Path $env:TEMP "skate-share-$stamp"
New-Item -ItemType Directory -Force -Path $outDir, $stage | Out-Null
try {
    foreach ($item in $pending) { Copy-Scrubbed $item $stage }
    $zip = Join-Path $outDir "session-$stamp.zip"
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal
} finally {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
}
$parts = @(Split-Zip $zip $PartMB)
$pending | ForEach-Object { Add-Content -Path $sharedFile -Value $_.FullName }
$names = ($parts | ForEach-Object { Split-Path $_ -Leaf }) -join ', '
$body = Protect-Text ("Build: $(Get-Build)`nFiles: $names (drag them into this issue)`n`nWhat happened:`n$Notes")
$url = Get-IssueUrl $slug "Session report $stamp" $body
Write-Host "Packed $($pending.Count) item(s) into $names in $outDir"
if (-not $NoOpen) {
    Start-Process $url
    Start-Process explorer.exe "/select,`"$($parts[0])`""
}
$url
