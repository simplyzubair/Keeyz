# Keeyz installer
#   irm https://keeyz.skillsgeek.com/install.ps1 | iex
#
# Downloads the newest Keeyz for Windows from the official GitHub releases,
# installs it for the current user (no admin needed) and starts it.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$url  = 'https://github.com/simplyzubair/Keeyz/releases/latest/download/Keeyz-setup.exe'
$file = Join-Path $env:TEMP 'Keeyz-setup.exe'

Write-Host ''
Write-Host '  Keeyz - AI that types where you type' -ForegroundColor White
Write-Host ''
Write-Host '  Downloading the latest version...'
Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing

Write-Host "  Downloaded $([math]::Round((Get-Item $file).Length / 1MB, 1)) MB"

Write-Host '  Installing...'
Get-Process keeyz -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Process -FilePath $file -ArgumentList '/S' -Wait
Remove-Item $file -Force -ErrorAction SilentlyContinue

$exe = Join-Path $env:LOCALAPPDATA 'Keeyz\keeyz.exe'
if (Test-Path $exe) {
  Start-Process $exe
  Write-Host ''
  Write-Host '  Done. Keeyz is running in your system tray.' -ForegroundColor Green
  Write-Host '  Select text anywhere and press Ctrl+Shift+A.'
  Write-Host ''
} else {
  Write-Host '  Something went wrong. Download it by hand: https://keeyz.skillsgeek.com/download' -ForegroundColor Yellow
}
