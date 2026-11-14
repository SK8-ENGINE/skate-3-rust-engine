# Main PC, as administrator: undo setup-main-pc.ps1. Removes the share and the account and turns the file sharing
# firewall rule off again. Keeps the folder and the logs in it, and leaves the network type as it is (set it back to
# Public in Settings > Network & internet if you want).
param(
    [string]$Account = 'skatelogs',
    [string]$Share = 'SkateSessions'
)
$ErrorActionPreference = 'Stop'
if (Get-SmbShare -Name $Share -ErrorAction SilentlyContinue) { Remove-SmbShare -Name $Share -Force; Write-Host "Removed share $Share." }
if (Get-LocalUser -Name $Account -ErrorAction SilentlyContinue) { Remove-LocalUser -Name $Account; Write-Host "Removed account $Account." }
Remove-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList' `
    -Name $Account -ErrorAction SilentlyContinue
Get-NetFirewallRule -DisplayGroup 'File and Printer Sharing' -Direction Inbound |
    Where-Object { $_.DisplayName -match 'SMB-In' } | Disable-NetFirewallRule
Write-Host 'File and Printer Sharing (SMB-In) disabled. The folder and its logs are kept.'
