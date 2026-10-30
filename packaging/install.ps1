# Per-user install of aiterm (no admin needed).
# Adds: Start Menu entry (Windows search), PATH entry, and an "Installed apps" uninstall entry.
# -UseWindowsTerminal: open in Windows Terminal instead (nicer rendering, but the taskbar then
# shows Terminal's icon instead of aiterm's).
param([switch]$Quiet, [switch]$UseWindowsTerminal)
$ErrorActionPreference = 'Stop'

$src     = $PSScriptRoot
$dest    = Join-Path $env:LOCALAPPDATA 'Programs\aiterm'
$version = '0.1.0'

if (-not (Test-Path (Join-Path $src 'aiterm.exe'))) { throw "aiterm.exe not found next to install.ps1 (run build.ps1 first)" }

Get-Process aiterm -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dest*" } | Stop-Process -Force
New-Item -ItemType Directory -Force $dest | Out-Null
foreach ($f in 'aiterm.exe', 'aiterm.ico', 'uninstall.ps1') { Copy-Item (Join-Path $src $f) $dest -Force }
$exe = Join-Path $dest 'aiterm.exe'
$ico = Join-Path $dest 'aiterm.ico'

# Start Menu shortcut: this is what the Windows key search finds.
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'aiterm.lnk'
$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk)
if ($UseWindowsTerminal) {
    $s.TargetPath = $exe
} else {
    # Launching through conhost gives aiterm its own window, so the taskbar shows aiterm's icon.
    $s.TargetPath = Join-Path $env:SystemRoot 'System32\conhost.exe'
    $s.Arguments  = "`"$exe`""
}
$s.WorkingDirectory = $env:USERPROFILE
$s.IconLocation     = "$ico,0"
$s.Description      = 'AI terminal: bash + natural language commands (DeepSeek)'
$s.Save()

# PATH, so `aiterm` also works from any terminal.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $dest) {
    [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';'), $dest) -join ';').TrimStart(';'), 'User')
}

# Settings > Apps > Installed apps entry.
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\aiterm'
New-Item -Force $key | Out-Null
$props = @{
    DisplayName     = 'aiterm'
    DisplayVersion  = $version
    Publisher       = 'aiterm'
    DisplayIcon     = $ico
    InstallLocation = $dest
    UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$dest\uninstall.ps1`""
    NoModify        = 1
    NoRepair        = 1
    EstimatedSize   = [int]((Get-Item $exe).Length / 1KB)
}
foreach ($k in $props.Keys) { Set-ItemProperty $key $k $props[$k] }

# DeepSeek key, if none is configured yet.
$hasKey = [Environment]::GetEnvironmentVariable('DEEPSEEK_API_KEY', 'User') -or (Test-Path (Join-Path $env:USERPROFILE '.aiterm_key'))
if (-not $hasKey -and -not $Quiet) {
    $k = Read-Host 'DeepSeek API key (Enter to skip; you can set DEEPSEEK_API_KEY later)'
    if ($k) { [Environment]::SetEnvironmentVariable('DEEPSEEK_API_KEY', $k.Trim(), 'User') }
}

Write-Host "aiterm $version installed to $dest"
Write-Host "Press the Windows key and type 'aiterm' to start it."
