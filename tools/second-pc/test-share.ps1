# Check that the share account can write to the share: connect with net use, write and delete a file, disconnect.
# Main PC: run with -Server set to its own IP address (Windows allows one login per server name, and your own login
# already uses the PC's name and 127.0.0.1). Second PC: -Server <main PC name or IP>.
param(
    [Parameter(Mandatory)][string]$Server,
    [string]$Share = 'SkateSessions',
    [string]$Account = 'skatelogs',
    # The account's computer (the main PC's name); defaults to -Server.
    [string]$Computer = $Server
)
$unc = "\\$Server\$Share"
$password = Read-Host "Password of $Computer\$Account" -AsSecureString
$plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($password))
& net.exe use $unc $plain "/user:$Computer\$Account" /persistent:no | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Host "SHARE_TEST failed: net use $unc returned $LASTEXITCODE (wrong password, or the share is not reachable)."; exit 1 }
try {
    $file = Join-Path $unc "share-test-$env:COMPUTERNAME.txt"
    Set-Content -LiteralPath $file -Value 'ok' -ErrorAction Stop
    Remove-Item -LiteralPath $file -ErrorAction Stop
    Write-Host "SHARE_TEST ok: $Computer\$Account can write to $unc."
} catch {
    Write-Host "SHARE_TEST failed: connected, but could not write: $($_.Exception.Message)"
} finally {
    & net.exe use $unc /delete /y | Out-Null
}
