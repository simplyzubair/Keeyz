# Keeyz command line: installs and updates the Keeyz app (Windows).
#   keeyz install     install Keeyz, or update it if it is old
#   keeyz update      same as install
#   keeyz version     show the installed and the newest version
#   keeyz cleanup     remove old apps that Keeyz replaces (older names of Keeyz, Snapperr)
param([string]$Command = "help")
$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12   # PowerShell 5.1 default is too old for GitHub
$Repo = "simplyzubair/Keeyz"
$UninstallKeys = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall", "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall", "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
# Apps Keeyz replaces: its older names, and Snapperr (window snapping is now built into Keeyz).
$OldApps = '^(Keyshifterr|Zkey|Snapperr)( [\d.]+)?$'

function Get-InstalledApps($namePattern) {
    foreach ($k in $UninstallKeys) {
        Get-ItemProperty "$k\*" -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -match $namePattern }
    }
}

function Get-Installed {
    $app = Get-InstalledApps '^Keeyz$' | Select-Object -First 1
    if ($app -and $app.DisplayVersion) { return [version]$app.DisplayVersion }
    return $null
}

function Get-Latest {
    $r = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "keeyz-cli" }
    $asset = $r.assets | Where-Object { $_.name -match '^Keeyz_[\d.]+_x64-setup\.exe$' } | Select-Object -First 1
    if (-not $asset) { throw "The newest release has no Windows installer yet. Try again in a few minutes." }
    if ($asset.digest -notmatch '^sha256:([0-9a-fA-F]{64})$') { throw "The newest release has no checksum, so it cannot be checked." }
    [pscustomobject]@{ Version = [version]($r.tag_name -replace '^v'); Url = $asset.browser_download_url; Sha256 = $Matches[1]; File = $asset.name }
}

# Quietly uninstall the old apps Keeyz replaces. Never touches Keeyz itself or its settings.
function Remove-OldApps {
    $old = @(Get-InstalledApps $OldApps)
    foreach ($a in $old) {
        # "C:\path\uninstaller.exe" /args  ->  path + args
        $s = "$($a.UninstallString)".Trim()
        if ($s -match '^"([^"]+)"\s*(.*)$') { $exe = $Matches[1]; $args0 = $Matches[2] } else { $exe = $s; $args0 = "" }
        if (-not (Test-Path $exe)) { continue }
        Get-Process -Name ($a.DisplayName -replace ' [\d.]+$', '') -ErrorAction SilentlyContinue | Stop-Process -Force
        $quiet = if ((Split-Path $exe -Leaf) -match '^unins') { "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART" }   # Inno Setup (Snapperr)
                 else { "/S" }                                                                                # NSIS (older Keeyz names)
        "Removing $($a.DisplayName) (Keeyz replaces it)..."
        $p = Start-Process $exe -ArgumentList "$args0 $quiet".Trim() -PassThru
        $p.WaitForExit()
        Start-Sleep -Seconds 2   # uninstallers often finish in a copy of themselves
    }
    if (-not $old.Count) { "No old apps to remove." }
}

function Install-Latest {
    $have = Get-Installed
    $new = Get-Latest
    Remove-OldApps
    if ($have -and $have -ge $new.Version) { "Keeyz $have is already the newest version."; return }
    if (-not $new.Url.StartsWith("https://github.com/$Repo/releases/download/")) { throw "Bad download link: $($new.Url)" }
    $setup = Join-Path $env:TEMP $new.File
    "Downloading Keeyz $($new.Version)..."
    $ProgressPreference = "SilentlyContinue"   # the progress bar makes downloads in PowerShell 5.1 very slow
    Invoke-WebRequest $new.Url -OutFile $setup -UseBasicParsing
    try {
        if ((Get-FileHash $setup -Algorithm SHA256).Hash -ne $new.Sha256.ToUpper()) { throw "The download is damaged (checksum does not match). Please try again." }
        # the running app would block replacing its files; the new version replaces the old one in place
        Get-Process keeyz -ErrorAction SilentlyContinue | Stop-Process -Force
        Start-Sleep -Milliseconds 800
        "Installing..."
        $p = Start-Process $setup -ArgumentList "/S" -PassThru
        $p.WaitForExit()
        if ($p.ExitCode -ne 0) { throw "The installer failed (exit code $($p.ExitCode))." }
    } finally { Remove-Item $setup -ErrorAction SilentlyContinue }
    $exe = "$env:LOCALAPPDATA\Keeyz\keeyz.exe"
    if (Test-Path $exe) { Start-Process $exe }
    if ($have) { "Updated Keeyz $have -> $($new.Version). Your settings and keys are kept." } else { "Installed Keeyz $($new.Version). It is in the tray (bottom right)." }
}

trap {
    $m = "$_"
    if ($m -match "rate limit") { $m = "GitHub is busy for this network (too many checks this hour). Please try again later." }
    Write-Host "Keeyz: $m" -ForegroundColor Red
    exit 1
}

switch ($Command) {
    { $_ -in "install", "update" } { Install-Latest }
    "version" {
        $have = Get-Installed
        $new = (Get-Latest).Version
        "Installed: " + $(if ($have) { $have } else { "not installed" })
        "Newest:    $new"
        if ($have -and $new -gt $have) { Write-Host "Update available: run 'keeyz update'" -ForegroundColor Yellow }
        elseif (-not $have) { "Run 'keeyz install' to install it." }
        else { "You have the newest version." }
    }
    "cleanup" { Remove-OldApps }
    default {
        "Keeyz command line"
        "  keeyz install     install Keeyz, or update it if it is old"
        "  keeyz update      same as install"
        "  keeyz version     show the installed and the newest version"
        "  keeyz cleanup     remove old apps Keeyz replaces (older Keeyz names, Snapperr)"
    }
}
