# Main PC, run once as administrator: lets a second PC on the home network drop session logs into one folder.
# Creates a local account that can only write to that folder, shares the folder over SMB (Windows file sharing),
# sets the wired / Wi-Fi network to Private and allows file sharing through the firewall on Private networks only.
# Undo with remove-main-pc.ps1. Guide: docs/hails-additions/second-pc-testing.md#sending-sessions-back
param(
    # The folder the logs land in (created if missing).
    [Parameter(Mandatory)][string]$Folder,
    [string]$Account = 'skatelogs',
    [string]$Share = 'SkateSessions',
    # Network adapter whose network becomes Private (Get-NetConnectionProfile lists them).
    [string]$InterfaceAlias = 'Ethernet'
)
$ErrorActionPreference = 'Stop'
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this as administrator.'
}
$computer = $env:COMPUTERNAME
New-Item -ItemType Directory -Force -Path $Folder | Out-Null
$Folder = (Resolve-Path $Folder).ProviderPath

# 1. The account: no group membership, hidden from the sign-in screen, password chosen here.
if (Get-LocalUser -Name $Account -ErrorAction SilentlyContinue) {
    Write-Host "Account $Account exists already: keeping it and its password."
    $password = $null
} else {
    $password = Read-Host "Choose a password for $computer\$Account (you type it again once on the second PC)" -AsSecureString
    $again = Read-Host 'Same password again' -AsSecureString
    $plain = { param($s) [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($s)) }
    if ((& $plain $password) -ne (& $plain $again)) { throw 'The passwords differ; nothing was changed.' }
    if ((& $plain $password).Length -lt 8) { throw 'Use at least 8 characters; nothing was changed.' }
    New-LocalUser -Name $Account -Password $password -PasswordNeverExpires -UserMayNotChangePassword `
        -AccountNeverExpires -Description 'Session logs from a second PC (one share)' | Out-Null
    $hide = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList'
    New-Item -Force -Path $hide | Out-Null
    New-ItemProperty -Force -Path $hide -Name $Account -Value 0 -PropertyType DWord | Out-Null
    Write-Host "Created $computer\$Account (hidden from the sign-in screen)."
}

# 2. Folder rights: the account may create and change files in this folder only.
& icacls $Folder /grant "${computer}\${Account}:(OI)(CI)M" | Out-Null
Write-Host "Granted $Account modify on $Folder."

# 3. The share: only this account, change access.
if (Get-SmbShare -Name $Share -ErrorAction SilentlyContinue) { Remove-SmbShare -Name $Share -Force }
New-SmbShare -Name $Share -Path $Folder -ChangeAccess "$computer\$Account" `
    -Description 'Session logs from a second PC' | Out-Null
Write-Host "Shared $Folder as \\$computer\$Share (only $Account)."

# 4. Network: Private, and file sharing allowed on Private networks only.
Set-NetConnectionProfile -InterfaceAlias $InterfaceAlias -NetworkCategory Private
Get-NetFirewallRule -DisplayGroup 'File and Printer Sharing' -Direction Inbound |
    Where-Object { $_.DisplayName -match 'SMB-In' -and $_.Profile -match 'Private' } |
    Enable-NetFirewallRule
Write-Host "Network on $InterfaceAlias set to Private; File and Printer Sharing (SMB-In) enabled for Private."

$ip = (Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias $InterfaceAlias | Select-Object -First 1).IPAddress
Write-Host "SETUP_DONE second PC uses: \\$computer\$Share (or \\$ip\$Share), user $computer\$Account"
Write-Host "Check the login: test-share.ps1 -Server $ip -Computer $computer (asks for the password)."
