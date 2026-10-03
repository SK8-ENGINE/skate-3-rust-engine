# Helpers for hosts where built-in cmdlets or .NET APIs are unavailable.
function Get-Sha256Hex([Parameter(Mandatory)][string]$LiteralPath) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $stream = [System.IO.File]::OpenRead($LiteralPath)
        try {
            return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLower()
        } finally { $stream.Dispose() }
    } finally { $sha.Dispose() }
}
function Get-RelativePath([Parameter(Mandatory)][string]$Base, [Parameter(Mandatory)][string]$Path) {
    $baseUri = New-Object System.Uri (($Base.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar))
    $pathUri = New-Object System.Uri $Path
    return [Uri]::UnescapeDataString($baseUri.MakeRelativeUri($pathUri).ToString()).Replace('/', [IO.Path]::DirectorySeparatorChar)
}
function Copy-IfExists([Parameter(Mandatory)][string[]]$Source, [Parameter(Mandatory)][string]$Destination) {
    foreach ($path in $Source) {
        if (Test-Path -LiteralPath $path) {
            Copy-Item -LiteralPath $path -Destination $Destination
        } else {
            Write-Warning "Skipping missing release file: $path"
        }
    }
}
function Copy-IfExists([Parameter(Mandatory)][string[]]$Source, [Parameter(Mandatory)][string]$Destination) {
    foreach ($path in $Source) {
        if (Test-Path -LiteralPath $path) {
            Copy-Item -LiteralPath $path -Destination $Destination
        }
    }
}
