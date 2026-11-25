# One-line setup for the Keeyz command line (Windows). In PowerShell:
#   irm https://raw.githubusercontent.com/simplyzubair/Keeyz/main/cli/install.ps1 | iex
# Puts the "keeyz" command in %LOCALAPPDATA%\Programs\Keeyz-cli (no admin), adds it to your
# PATH, then installs or updates the Keeyz app and removes the old apps it replaces.
$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$dir = "$env:LOCALAPPDATA\Programs\Keeyz-cli"
New-Item -ItemType Directory -Force $dir | Out-Null
$ProgressPreference = "SilentlyContinue"
Invoke-WebRequest "https://raw.githubusercontent.com/simplyzubair/Keeyz/main/cli/keeyz.ps1" -OutFile "$dir\keeyz.ps1" -UseBasicParsing
Set-Content "$dir\keeyz.cmd" '@powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0keeyz.ps1" %*' -Encoding Ascii

$path = [Environment]::GetEnvironmentVariable("Path", "User")
if (($path -split ";") -notcontains $dir) {
    [Environment]::SetEnvironmentVariable("Path", ($(if ($path) { "$path;" }) + $dir), "User")
}
if (($env:Path -split ";") -notcontains $dir) { $env:Path += ";$dir" }

& powershell -NoProfile -ExecutionPolicy Bypass -File "$dir\keeyz.ps1" install
"The 'keeyz' command is ready (open a new terminal to use it): keeyz update / keeyz version"
