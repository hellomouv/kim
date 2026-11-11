# Step 34 (ADR 0067): fetch the SSH server the SSH tests run against.
#
# Downloads Microsoft's portable Win32-OpenSSH release (pinned version and SHA-256) into
# target\test-sshd\ — nothing is installed, no service is created, nothing outside the
# repository's target folder changes. The tests start sshd.exe as the current user on
# 127.0.0.1 with a throwaway host key and key-only login. Run it by hand (or in the Daily
# workflow); a test run never downloads anything.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fetch-test-sshd.ps1
$ErrorActionPreference = 'Stop'
$version = '10.4.0.0p1-Preview'
$sha256 = 'b67ff8becd203502190ec42ac7787c24c2cd3325ed7700485fe7a1c83d2a7957'
$url = "https://github.com/PowerShell/Win32-OpenSSH/releases/download/$version/OpenSSH-Win64.zip"
$root = Join-Path (Split-Path -Parent $PSScriptRoot) 'target\test-sshd'
if (Test-Path (Join-Path $root 'OpenSSH-Win64\sshd.exe')) {
    Write-Output "already present: $root\OpenSSH-Win64"
    exit 0
}
New-Item -ItemType Directory -Force $root | Out-Null
$zip = Join-Path $root 'OpenSSH-Win64.zip'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
$got = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
if ($got -ne $sha256) {
    Remove-Item $zip
    throw "SHA-256 mismatch: expected $sha256, got $got"
}
Expand-Archive -Path $zip -DestinationPath $root -Force
Remove-Item $zip
Write-Output "ready: $root\OpenSSH-Win64 ($version)"
