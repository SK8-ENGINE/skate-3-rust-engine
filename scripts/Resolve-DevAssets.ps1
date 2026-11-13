function Resolve-DevAssets {
    param([Parameter(Mandatory)][string]$ProjectRoot, [string]$Assets)
    # Explicit overrides must fail clearly instead of silently selecting another install.
    $override = if ($Assets) { $Assets } else { $env:SKATE3_ASSETS }
    if ($override) {
        if (-not (Test-Path -LiteralPath (Join-Path $override 'private/game.json') -PathType Leaf)) {
            throw "Prepared assets not found at: $override"
        }
        return (Resolve-Path -LiteralPath $override).ProviderPath
    }
    $local = Join-Path $ProjectRoot 'assets'
    if (Test-Path -LiteralPath (Join-Path $local 'private/game.json') -PathType Leaf) {
        # The game canonicalizes junctions before locating map/settings siblings.
        return (Get-Item -LiteralPath $local).FullName
    }
    # A stale junction (a refresh published a new data\installations\<id> and deleted the old one, so every other
    # checkout's link points at nothing): re-point it at the installation its data folder names now.
    $link = Get-Item -LiteralPath $local -Force -ErrorAction SilentlyContinue
    if ($link -and $link.LinkType -eq 'Junction') {
        $oldTarget = @($link.Target)[0]
        $dataBase = Split-Path (Split-Path (Split-Path $oldTarget -Parent) -Parent) -Parent
        $marker = Join-Path $dataBase 'installation.json'
        if (Test-Path -LiteralPath $marker -PathType Leaf) {
            $installation = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
            if ($installation.directory -match '^installations/[0-9a-f]{32}$') {
                $current = Join-Path $dataBase ($installation.directory + '/assets')
                if (Test-Path -LiteralPath (Join-Path $current 'private/game.json') -PathType Leaf) {
                    cmd /c rmdir "$local" | Out-Null
                    New-Item -ItemType Junction -Path $local -Target $current | Out-Null
                    Write-Host "Re-pointed the stale assets junction to $current"
                    return (Get-Item -LiteralPath $local).FullName
                }
            }
        }
    }
    $parent = Split-Path $ProjectRoot -Parent
    $bases = @((Join-Path $ProjectRoot 'data'), (Join-Path $parent 'data'),
        (Join-Path $parent 'skate3rust-windows-x64/data'))
    foreach ($base in $bases) {
        $marker = Join-Path $base 'installation.json'
        if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) { continue }
        $installation = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
        if ($installation.directory -notmatch '^installations/[0-9a-f]{32}$') {
            throw "Invalid installation directory in $marker"
        }
        $candidate = Join-Path $base ($installation.directory + '/assets')
        if (Test-Path -LiteralPath (Join-Path $candidate 'private/game.json') -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).ProviderPath
        }
    }
    throw 'Prepared assets are missing. Put the installed skate3rust-windows-x64 folder beside this checkout, repair the assets junction, or set SKATE3_ASSETS to the converted assets folder.'
}
