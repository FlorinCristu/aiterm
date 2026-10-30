# Builds a release binary and stages a shareable dist/aiterm-setup folder + zip.
$ErrorActionPreference = 'Stop'
$root   = Split-Path $PSScriptRoot -Parent
# Short target dir: MSVC tools fail on very long paths.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $env:USERPROFILE '.cache\aiterm-target' }
$env:CARGO_TARGET_DIR = $target

Push-Location $env:USERPROFILE
# cargo reports progress on stderr, which Windows PowerShell turns into errors under 'Stop'.
$ErrorActionPreference = 'Continue'
try { cargo build --release --manifest-path (Join-Path $root 'Cargo.toml') 2>&1 | ForEach-Object { "$_" } }
finally { Pop-Location }
if ($LASTEXITCODE) { throw 'cargo build failed' }
$ErrorActionPreference = 'Stop'

$dist = Join-Path $root 'dist\aiterm-setup'
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item (Join-Path $target 'release\aiterm.exe') $dist
foreach ($f in 'aiterm.ico', 'install.ps1', 'uninstall.ps1', 'install.cmd') { Copy-Item (Join-Path $PSScriptRoot $f) $dist }

$zip = Join-Path $root 'dist\aiterm-setup.zip'
Compress-Archive -Path "$dist\*" -DestinationPath $zip -Force
Write-Host "Staged $dist"
Write-Host "Zip:   $zip   (unzip anywhere, double-click install.cmd)"
