# Per-user install of aiterm (no admin needed).
# Adds: Start Menu entry (Windows search), PATH entry, and an "Installed apps" uninstall entry.
# The shortcut opens aiterm-app.exe, aiterm's own window; `aiterm` on PATH is the console program.
# -Console: point the shortcut at aiterm.exe in a classic console window instead.
# -Quiet:   don't ask for a DeepSeek key (aiterm asks on first use).
# -WhatIf:  print what would change and change nothing.
[CmdletBinding(SupportsShouldProcess)]
param([switch]$Quiet, [switch]$Console)
$ErrorActionPreference = 'Stop'
# One plain line on stderr instead of PowerShell's error record: aiterm-setup.exe shows it as is.
trap { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }

$src     = $PSScriptRoot
$dest    = Join-Path $env:LOCALAPPDATA 'Programs\aiterm'
$version = '0.1.0'
$files   = 'aiterm.exe', 'aiterm-app.exe', 'busybox.exe', 'THIRD-PARTY.md', 'LICENSE', 'uninstall.ps1'

$missing = $files | Where-Object { -not (Test-Path (Join-Path $src $_)) }
if ($missing) { throw "Missing next to install.ps1: $($missing -join ', ') (run packaging\build.ps1 first)" }

# Running files can't be replaced; stop before copying anything rather than leave a mix of versions.
$running = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dest\*" }
if ($running) {
    throw "aiterm is running ($(($running.Name | Sort-Object -Unique) -join ', ')). Close all aiterm windows and try again."
}

New-Item -ItemType Directory -Force $dest | Out-Null
foreach ($f in $files) { Copy-Item (Join-Path $src $f) $dest -Force }
Remove-Item (Join-Path $dest 'aiterm.ico') -Force -ErrorAction SilentlyContinue # left by older versions
$exe = Join-Path $dest 'aiterm.exe'
$app = Join-Path $dest 'aiterm-app.exe'

# Start Menu shortcut: this is what the Windows key search finds.
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'aiterm.lnk'
if ($PSCmdlet.ShouldProcess($lnk, 'Create shortcut')) {
    $s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    if ($Console) {
        # Through conhost aiterm gets its own console window, so the taskbar shows aiterm's icon.
        $s.TargetPath   = Join-Path $env:SystemRoot 'System32\conhost.exe'
        $s.Arguments    = "`"$exe`""
        $s.IconLocation = "$exe,0"
    } else {
        $s.TargetPath   = $app
        $s.Arguments    = ''
        $s.IconLocation = "$app,0"
    }
    $s.WorkingDirectory = $env:USERPROFILE
    $s.Description      = 'AI terminal: bash + natural language commands'
    $s.Save()
}

# PATH, so `aiterm` also works from any terminal.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $dest -and $PSCmdlet.ShouldProcess('user PATH', "Add $dest")) {
    [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';'), $dest) -join ';').TrimStart(';'), 'User')
}

# Settings > Apps > Installed apps entry.
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\aiterm'
$size = ($files | ForEach-Object { (Get-Item (Join-Path $src $_)).Length } | Measure-Object -Sum).Sum
$props = @{
    DisplayName     = 'aiterm'
    DisplayVersion  = $version
    Publisher       = 'aiterm'
    DisplayIcon     = $app
    InstallLocation = $dest
    UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$dest\uninstall.ps1`""
    NoModify        = 1
    NoRepair        = 1
    EstimatedSize   = [int]($size / 1KB)
}
if ($PSCmdlet.ShouldProcess($key, 'Write uninstall entry')) {
    New-Item -Force $key | Out-Null
    foreach ($k in $props.Keys) { Set-ItemProperty $key $k $props[$k] }
}

# DeepSeek key, if none is configured yet.
$hasKey = [Environment]::GetEnvironmentVariable('DEEPSEEK_API_KEY', 'User') -or (Test-Path (Join-Path $env:USERPROFILE '.aiterm_key'))
if (-not $hasKey -and -not $Quiet -and -not $WhatIfPreference) {
    $k = Read-Host 'DeepSeek API key (Enter to skip; aiterm asks again when you first use the AI)'
    if ($k) { [Environment]::SetEnvironmentVariable('DEEPSEEK_API_KEY', $k.Trim(), 'User') }
}

Write-Host "aiterm $version installed to $dest"
Write-Host "Press the Windows key and type 'aiterm' to start it."
