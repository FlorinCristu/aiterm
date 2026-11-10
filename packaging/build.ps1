# Builds everything a Windows release ships, into dist\:
#   dist\aiterm-setup\      the staged files (exes, busybox, notices, install scripts)
#   dist\aiterm-setup.exe   single-file installer with those files embedded (the main download)
#   dist\aiterm-setup.zip   the same files zipped: unzip, double-click install.cmd
$ErrorActionPreference = 'Stop'
$root   = Split-Path $PSScriptRoot -Parent
# Short target dir: MSVC tools fail on very long paths.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $env:USERPROFILE '.cache\aiterm-target' }
$env:CARGO_TARGET_DIR = $target
$release = Join-Path $target 'release'

function Cargo-Build([string[]]$packages) {
    $cargoArgs = @('build', '--release', '--manifest-path', (Join-Path $root 'Cargo.toml'))
    foreach ($p in $packages) { $cargoArgs += '-p', $p }
    Push-Location $env:USERPROFILE
    # cargo reports progress on stderr, which Windows PowerShell turns into errors under 'Stop'.
    $ErrorActionPreference = 'Continue'
    try { cargo @cargoArgs 2>&1 | ForEach-Object { "$_" } }
    finally { Pop-Location; $ErrorActionPreference = 'Stop' }
    if ($LASTEXITCODE) { throw "cargo build $packages failed" }
}

Cargo-Build 'aiterm', 'aiterm-app'

$dist  = Join-Path $root 'dist'
$stage = Join-Path $dist 'aiterm-setup'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in 'aiterm.exe', 'aiterm-app.exe') { Copy-Item (Join-Path $release $f) $stage }
# Cached next to the build, so a rebuild doesn't download it again.
$bbDir = Join-Path $target 'busybox'
& (Join-Path $PSScriptRoot 'fetch-busybox.ps1') -Dest $bbDir
Copy-Item (Join-Path $bbDir 'busybox.exe') $stage
foreach ($f in 'THIRD-PARTY.md', 'install.ps1', 'uninstall.ps1', 'install.cmd') { Copy-Item (Join-Path $PSScriptRoot $f) $stage }
Copy-Item (Join-Path $root 'LICENSE') $stage

# The installer embeds whatever is in $stage (see setup\build.rs).
$env:AITERM_PAYLOAD = $stage
try { Cargo-Build 'aiterm-setup' } finally { Remove-Item Env:AITERM_PAYLOAD }
$exe = Join-Path $dist 'aiterm-setup.exe'
Copy-Item (Join-Path $release 'aiterm-setup.exe') $exe -Force

$zip = Join-Path $dist 'aiterm-setup.zip'
Compress-Archive -Path "$stage\*" -DestinationPath $zip -Force

Write-Host "Staged    $stage"
Write-Host ("Installer $exe  ({0:N1} MB)" -f ((Get-Item $exe).Length / 1MB))
Write-Host ("Zip       $zip  ({0:N1} MB; unzip, double-click install.cmd)" -f ((Get-Item $zip).Length / 1MB))
