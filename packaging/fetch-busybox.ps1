# Puts busybox-w32 (64-bit Unicode build) at <Dest>\busybox.exe, the built-in shell aiterm falls
# back to on Windows when Git Bash is not installed. Pinned version + SHA-256; does nothing if the
# right file is already there. busybox is GPL-2.0: ship packaging\THIRD-PARTY.md alongside it.
#   powershell -ExecutionPolicy Bypass -File packaging\fetch-busybox.ps1 -Dest target\debug
param([string]$Dest = (Join-Path $PSScriptRoot '..\target\debug'))

$ErrorActionPreference = 'Stop'
$Version = 'FRP-6354-g5f09fb446'
$Url = "https://frippery.org/files/busybox/busybox-w64u-$Version.exe"
$Sha256 = '36B266923FC5765B42F5F981319F39568498FBA60B53CCF26E8FC1E575E2A9A9'

New-Item -ItemType Directory -Force $Dest | Out-Null
$out = Join-Path (Resolve-Path $Dest) 'busybox.exe'
if ((Test-Path $out) -and (Get-FileHash $out -Algorithm SHA256).Hash -eq $Sha256) {
    Write-Host "busybox $Version already in $out"
    exit 0
}

$tmp = "$out.download"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
$got = (Get-FileHash $tmp -Algorithm SHA256).Hash
if ($got -ne $Sha256) {
    Remove-Item $tmp -Force
    throw "busybox download checksum mismatch: expected $Sha256, got $got"
}
Move-Item $tmp $out -Force
Write-Host "busybox $Version -> $out"
