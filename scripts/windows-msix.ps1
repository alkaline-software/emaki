# The Microsoft Store package: Emaki.exe with the manifest and tile images
# of crates/emaki-app/windows/store, as an .msix for Partner Center, which
# signs it. The identity is the one Partner Center reserved for the app
# (Product management > Product identity).
#
#   scripts/windows-msix.ps1 -Exe target/release/Emaki.exe -Version 0.3.1 -Arch x64 `
#     -Name <Package/Identity/Name> -Publisher <Package/Identity/Publisher> `
#     -PublisherName <Package/Properties/PublisherDisplayName> -Out Emaki-windows-x64.msix
param(
  [Parameter(Mandatory)] [string] $Exe,
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [ValidateSet('x64', 'arm64')] [string] $Arch,
  [Parameter(Mandatory)] [string] $Name,
  [Parameter(Mandatory)] [string] $Publisher,
  [Parameter(Mandatory)] [string] $PublisherName,
  [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$store = Join-Path $root 'crates/emaki-app/windows/store'
$dir = Join-Path ([IO.Path]::GetTempPath()) "emaki-msix-$Arch"
Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $Exe (Join-Path $dir 'Emaki.exe')
Copy-Item -Recurse (Join-Path $store 'Assets') (Join-Path $dir 'Assets')
# A package's version has four parts, the last 0 for the Store.
$manifest = (Get-Content -Raw (Join-Path $store 'AppxManifest.xml')).
  Replace('{{NAME}}', $Name).Replace('{{PUBLISHER}}', [Security.SecurityElement]::Escape($Publisher)).
  Replace('{{PUBLISHER_NAME}}', [Security.SecurityElement]::Escape($PublisherName)).
  Replace('{{VERSION}}', "$Version.0").Replace('{{ARCH}}', $Arch)
Set-Content -Encoding utf8 (Join-Path $dir 'AppxManifest.xml') $manifest
# makeappx is the Windows SDK's; the newest one installed.
$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeappx) { throw 'makeappx.exe not found: install the Windows SDK' }
& $makeappx.FullName pack /o /d $dir /p $Out
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Remove-Item -Recurse -Force $dir
Write-Host "wrote $Out"
