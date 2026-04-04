#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Configures a Windows machine for HoloBridge cross-machine development.

.DESCRIPTION
    Sets up:
    - OpenSSH Server (for remote builds from Mac)
    - Windows Firewall rule for QUIC (UDP 4433)
    - Displays LAN IP addresses for client configuration
    - Verifies Rust toolchain is available

.PARAMETER Port
    UDP port for HoloBridge QUIC transport. Default: 4433.

.PARAMETER SkipSSH
    Skip OpenSSH Server setup.

.EXAMPLE
    # Run from an elevated PowerShell prompt:
    .\scripts\setup-windows-dev.ps1
#>
param(
    [int]$Port = 4433,
    [switch]$SkipSSH
)

$ErrorActionPreference = 'Stop'

Write-Host "`n=== HoloBridge Windows Dev Setup ===" -ForegroundColor Cyan
Write-Host ""

# --- 1. OpenSSH Server ---
if (-not $SkipSSH) {
    Write-Host "[1/4] OpenSSH Server" -ForegroundColor Yellow

    $sshCapability = Get-WindowsCapability -Online | Where-Object Name -like 'OpenSSH.Server*'

    if ($sshCapability.State -eq 'Installed') {
        Write-Host "  Already installed." -ForegroundColor Green
    } else {
        Write-Host "  Installing OpenSSH Server..."
        Add-WindowsCapability -Online -Name 'OpenSSH.Server~~~~0.0.1.0' | Out-Null
        Write-Host "  Installed." -ForegroundColor Green
    }

    $sshdService = Get-Service -Name sshd -ErrorAction SilentlyContinue
    if ($sshdService -and $sshdService.Status -eq 'Running') {
        Write-Host "  sshd service is running." -ForegroundColor Green
    } else {
        Write-Host "  Starting sshd service..."
        Set-Service -Name sshd -StartupType Automatic
        Start-Service sshd
        Write-Host "  sshd started and set to automatic." -ForegroundColor Green
    }

    # Ensure the SSH firewall rule exists (Windows usually creates it on install, but verify)
    $sshRule = Get-NetFirewallRule -DisplayName 'OpenSSH Server (sshd)' -ErrorAction SilentlyContinue
    if (-not $sshRule) {
        $sshRule = Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue
    }
    if ($sshRule) {
        Write-Host "  SSH firewall rule exists." -ForegroundColor Green
    } else {
        Write-Host "  Creating SSH firewall rule..."
        New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server (sshd)' `
            -Direction Inbound -Action Allow -Protocol TCP -LocalPort 22 | Out-Null
        Write-Host "  SSH firewall rule created." -ForegroundColor Green
    }
} else {
    Write-Host "[1/4] OpenSSH Server — skipped" -ForegroundColor DarkGray
}

# --- 2. QUIC Firewall Rule ---
Write-Host ""
Write-Host "[2/4] QUIC Firewall Rule (UDP $Port)" -ForegroundColor Yellow

$ruleName = "HoloBridge QUIC (UDP $Port)"
$existingRule = Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue

if ($existingRule) {
    Write-Host "  Rule '$ruleName' already exists." -ForegroundColor Green
} else {
    New-NetFirewallRule -DisplayName $ruleName `
        -Direction Inbound -Action Allow -Protocol UDP -LocalPort $Port `
        -Description 'Allow inbound QUIC traffic for HoloBridge transport' | Out-Null
    Write-Host "  Created firewall rule '$ruleName'." -ForegroundColor Green
}

# --- 3. Rust Toolchain ---
Write-Host ""
Write-Host "[3/4] Rust Toolchain" -ForegroundColor Yellow

$rustc = Get-Command rustc -ErrorAction SilentlyContinue
$cargo = Get-Command cargo -ErrorAction SilentlyContinue

if ($rustc -and $cargo) {
    $rustcVersion = & rustc --version
    $cargoVersion = & cargo --version
    Write-Host "  $rustcVersion" -ForegroundColor Green
    Write-Host "  $cargoVersion" -ForegroundColor Green
} else {
    Write-Host "  WARNING: Rust toolchain not found in PATH." -ForegroundColor Red
    Write-Host "  Install from https://rustup.rs" -ForegroundColor Red
}

# --- 4. Network Info ---
Write-Host ""
Write-Host "[4/4] LAN IP Addresses" -ForegroundColor Yellow

$adapters = Get-NetIPAddress -AddressFamily IPv4 |
    Where-Object { $_.IPAddress -ne '127.0.0.1' -and $_.PrefixOrigin -ne 'WellKnown' } |
    Sort-Object -Property InterfaceAlias

if ($adapters) {
    foreach ($a in $adapters) {
        $ifName = $a.InterfaceAlias
        Write-Host "  $($a.IPAddress)  ($ifName)" -ForegroundColor Green
    }
} else {
    Write-Host "  No LAN addresses found." -ForegroundColor Red
}

# --- Summary ---
Write-Host ""
Write-Host "=== Setup Complete ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "Next steps:" -ForegroundColor White
Write-Host "  1. From your Mac, test SSH:  ssh $env:USERNAME@<LAN_IP>"
Write-Host "  2. Build capture on Windows: .\scripts\host-capture-build.ps1"
Write-Host "  3. Smoke DXGI capture:       .\scripts\host-capture-smoke.ps1"
Write-Host "  4. Build H.264 encode:       .\scripts\host-encode-build.ps1"
Write-Host "  5. Smoke H.264 encode:       .\scripts\host-encode-smoke.ps1"
Write-Host "  6. Run QUIC server later:    .\scripts\host-transport-server.ps1 -Bind 0.0.0.0"
Write-Host "  5. Connect from Mac/AVP:     Use <LAN_IP>:$Port as the host address"
Write-Host ""
