# Build a signed Keeyz release and publish it, so installed apps can update themselves.
#
# Usage (from the project folder):
#   infisical run --env=prod -- powershell -File scripts\release.ps1 -Version 0.4.1 -Notes "What changed"
# Without Infisical it falls back to the key file in %USERPROFILE%\.tauri\keeyz.key.
#
# Steps: set version -> build -> make latest.json -> GitHub release with installer + signature + latest.json.
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$Notes = "Bug fixes and improvements."
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

# 1) signing key (never printed)
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
  $keyFile = Join-Path $env:USERPROFILE ".tauri\keeyz.key"
  if (-not (Test-Path $keyFile)) { throw "No signing key. Run via 'infisical run' or put the key in $keyFile" }
  $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $keyFile -Raw
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "" }

# 2) version everywhere (UTF-8 *without* BOM - the build tools can't read a BOM)
$utf8 = New-Object System.Text.UTF8Encoding($false)
function Set-Version($path, $pattern, $replacement) {
  $full = (Resolve-Path $path).Path
  $text = [IO.File]::ReadAllText($full)
  [IO.File]::WriteAllText($full, [regex]::Replace($text, $pattern, $replacement, 1), $utf8)
}
Set-Version "src-tauri\tauri.conf.json" '"version": "[^"]+"' "`"version`": `"$Version`""
Set-Version "package.json" '"version": "[^"]+"' "`"version`": `"$Version`""
Set-Version "src-tauri\Cargo.toml" '(?m)^version = "[^"]+"' "version = `"$Version`""

# 3) build
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npx tauri build
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

# 4) latest.json for the in-app updater
$dir = "src-tauri\target\release\bundle\nsis"
$exe = "Keeyz_${Version}_x64-setup.exe"
$sig = Get-Content (Join-Path $dir "$exe.sig") -Raw
$latest = [ordered]@{
  version   = $Version
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = @{ "windows-x86_64" = @{ signature = $sig.Trim(); url = "https://github.com/simplyzubair/Keeyz/releases/download/v$Version/$exe" } }
}
$latestPath = Join-Path $dir "latest.json"
[IO.File]::WriteAllText((Join-Path (Get-Location) $latestPath), ($latest | ConvertTo-Json -Depth 5), $utf8)

# 5) commit version bump + publish
git add -A
git commit -m "release: v$Version" | Out-Null
git push origin main
gh release create "v$Version" (Join-Path $dir $exe) (Join-Path $dir "$exe.sig") $latestPath --title "Keeyz v$Version" --notes $Notes
Write-Host "Published v$Version. Installed apps will offer the update."
