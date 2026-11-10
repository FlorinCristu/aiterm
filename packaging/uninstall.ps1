# Removes everything install.ps1 added, plus the window's WebView2 cache.
# Keeps your settings: ~/.aiterm_*, and DEEPSEEK_API_KEY.
# -WhatIf: print what would change and change nothing.
[CmdletBinding(SupportsShouldProcess)]
param()
$ErrorActionPreference = 'SilentlyContinue'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\aiterm'

# You asked to remove it, so close it (aiterm-app, its tabs' aiterm.exe and their busybox shells)
# rather than leave files that can't be deleted.
$running = Get-Process | Where-Object { $_.Path -like "$dest\*" }
if ($running) {
    Write-Host 'Closing aiterm...'
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 500 # let Windows release the files
}

Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'aiterm.lnk') -Force
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\aiterm' -Recurse -Force
$cache = Join-Path $env:LOCALAPPDATA 'aiterm' # aiterm-app's WebView2 data

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$kept = ($userPath -split ';') | Where-Object { $_ -and $_ -ne $dest }
if (($kept -join ';') -ne $userPath -and $PSCmdlet.ShouldProcess('user PATH', "Remove $dest")) {
    [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
}

# This script lives inside $dest, so delete the folder after it exits (and the cache once
# WebView2's helper processes have gone too).
if ($PSCmdlet.ShouldProcess("$dest, $cache", 'Delete folders')) {
    Start-Process cmd.exe -WindowStyle Hidden -ArgumentList "/c timeout /t 2 >nul & rmdir /s /q `"$dest`" & rmdir /s /q `"$cache`""
}
Write-Host 'aiterm uninstalled.'
