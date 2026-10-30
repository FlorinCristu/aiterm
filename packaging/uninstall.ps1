# Removes everything install.ps1 added. Keeps ~/.aiterm_history, ~/.aiterm_key and DEEPSEEK_API_KEY.
$ErrorActionPreference = 'SilentlyContinue'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\aiterm'

Get-Process aiterm | Where-Object { $_.Path -like "$dest*" } | Stop-Process -Force
Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'aiterm.lnk') -Force
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\aiterm' -Recurse -Force

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$kept = ($userPath -split ';') | Where-Object { $_ -and $_ -ne $dest }
[Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')

# This script lives inside $dest, so delete the folder after it exits.
Start-Process cmd.exe -WindowStyle Hidden -ArgumentList "/c timeout /t 2 >nul & rmdir /s /q `"$dest`""
Write-Host 'aiterm uninstalled.'
