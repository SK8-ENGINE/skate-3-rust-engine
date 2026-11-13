# Main PC: fetch session logs a second PC sent as pull requests (play-and-send.ps1's GitHub fallback). Every open PR
# whose branch starts with session-logs/ is downloaded and unpacked into <Folder>\<second PC name>\, then closed and its
# branch deleted. Needs the GitHub CLI, signed in.
param(
    # The shared session folder (the one setup-main-pc.ps1 shared).
    [Parameter(Mandatory)][string]$Folder,
    [string]$Repo = 'Hailey-Ross/rusty-trucks',
    # Also comment on and close each fetched session-report issue (otherwise they are only recorded as fetched).
    [switch]$Close
)
$ErrorActionPreference = 'Stop'

# Players' session reports: docs/hails-additions/second-pc-testing.md#share-sessions-via-github
$fetchedFile = Join-Path $Folder 'fetched-issues.txt'
$fetched = @{}
if (Test-Path $fetchedFile) { foreach ($n in Get-Content $fetchedFile) { $fetched[$n] = $true } }
# Through ForEach-Object: Windows PowerShell's ConvertFrom-Json returns an empty list as one object (the loop then ran
# once with no number and gh failed, 2026-10-22).
$issues = @(& gh issue list --repo $Repo --state open --label session-report --limit 100 --json number,author | ConvertFrom-Json | ForEach-Object { $_ } | Where-Object { $_.number })
foreach ($issue in $issues | Where-Object { -not $fetched["$($_.number)"] }) {
    $view = & gh issue view $issue.number --repo $Repo --json body,comments | ConvertFrom-Json
    $text = (@($view.body) + @($view.comments | ForEach-Object { $_.body })) -join "`n"
    $urls = @([regex]::Matches($text, 'https://github\.com/user-attachments/files/\d+/[^\s)\]]+\.zip(\.\d{3})?') | ForEach-Object { $_.Value } | Select-Object -Unique)
    if ($urls.Count -eq 0) { Write-Host "Issue #$($issue.number): no zip attached yet."; continue }
    $dest = Join-Path $Folder ("github-{0}-{1}" -f $issue.number, $issue.author.login)
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    foreach ($g in $urls | Group-Object { (Split-Path $_ -Leaf) -replace '\.\d{3}$', '' }) {
        $zip = Join-Path $env:TEMP ("issue-{0}-{1}" -f $issue.number, $g.Name)
        $out = [IO.File]::Create($zip)
        try {
            foreach ($u in $g.Group | Sort-Object) {
                $part = "$zip.part"
                Invoke-WebRequest -UseBasicParsing -Uri $u -OutFile $part
                $b = [IO.File]::ReadAllBytes($part); $out.Write($b, 0, $b.Length)
                Remove-Item $part
            }
        } finally { $out.Close() }
        Expand-Archive -LiteralPath $zip -DestinationPath $dest -Force
        Remove-Item $zip
        Write-Host "Issue #$($issue.number): unpacked $($g.Name) ($($g.Count) part(s)) into $dest"
    }
    Add-Content -Path $fetchedFile -Value "$($issue.number)"
    if ($Close) { & gh issue close $issue.number --repo $Repo --comment 'Thanks, the session was fetched.' | Out-Null; Write-Host "Issue #$($issue.number): closed." }
}

$prs = @(& gh pr list --repo $Repo --state open --limit 100 --json number,headRefName | ConvertFrom-Json | ForEach-Object { $_ } |
    Where-Object { $_.headRefName -like 'session-logs/*' })
if (-not $prs) { Write-Host 'FETCH_DONE no session-log pull requests open.'; return }
foreach ($pr in $prs) {
    $branch = $pr.headRefName
    # session-logs/<pc>-<yyyyMMdd>-<HHmmss>
    $pc = ($branch -replace '^session-logs/', '') -replace '-\d{8}-\d{6}$', ''
    $dest = Join-Path $Folder $pc.ToUpper()
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    $items = & gh api "repos/$Repo/contents/session-logs?ref=$branch" | ConvertFrom-Json
    # A zip comes whole (<name>.zip) or in parts (<name>.zip.001, .002, ...): join the parts in order.
    $groups = $items | Where-Object { $_.name -match '\.zip(\.\d{3})?$' } | Group-Object { $_.name -replace '\.\d{3}$', '' }
    foreach ($g in $groups) {
        $zip = Join-Path $env:TEMP $g.Name
        $out = [IO.File]::Create($zip)
        try {
            foreach ($item in $g.Group | Sort-Object name) {
                $partFile = Join-Path $env:TEMP ('part-' + $item.name)
                Invoke-WebRequest -UseBasicParsing -Uri $item.download_url -OutFile $partFile
                $b = [IO.File]::ReadAllBytes($partFile); $out.Write($b, 0, $b.Length)
                Remove-Item $partFile
            }
        } finally { $out.Close() }
        Expand-Archive -LiteralPath $zip -DestinationPath $dest -Force
        Remove-Item $zip
        Write-Host "PR #$($pr.number): unpacked $($g.Name) ($($g.Count) part(s)) into $dest"
    }
    & gh pr close $pr.number --repo $Repo --delete-branch --comment 'Fetched to the main PC.' | Out-Null
    Write-Host "PR #$($pr.number): closed, branch $branch deleted."
}
Write-Host 'FETCH_DONE'
