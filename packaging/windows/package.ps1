<#
.SYNOPSIS
  Build and package MusicCraft for Windows x64.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    musiccraft-<version>-windows-x64.msi            installer (WiX v5), for all users or one
    musiccraft-<version>-windows-x64-portable.zip   musiccraft.exe and the licence files

  The binary links the C runtime statically, so no Visual C++ redistributable is needed. The
  files are not signed: MusicCraft has no Windows code-signing certificate yet.

  Needs: Rust (MSVC toolchain) and WiX v5: dotnet tool install --global wix --version 5.0.2
  Adapted from storytold/craft-launcher (packaging/windows/package.ps1, MIT OR Apache-2.0).

.EXAMPLE
  pwsh packaging/windows/package.ps1
#>
param([switch] $SkipBuild)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Pkg = 'musiccraft'

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

$Version = $env:MUSICCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric: a pre-release tag is dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = 'x86_64-pc-windows-msvc'
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Write-Output "MusicCraft $Version for Windows x64"

if (-not $SkipBuild) {
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
  $env:MUSICCRAFT_REQUIRE_WINRES = '1'  # an executable without its icon fails the build
  Invoke-Native 'cargo build' { cargo build --release --locked -p $Pkg -p "$Pkg-cli" --target $Target }
}
$Exe = Join-Path $TargetDir "$Target\release\$Pkg.exe"
$Cli = Join-Path $TargetDir "$Target\release\$Pkg-cli.exe"

# The PE header must say x64 (machine 0x8664) and Windows GUI (subsystem 2): no console window.
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$pe = [BitConverter]::ToInt32($bytes, 0x3C)
$machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
$subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C)
if ($machine -ne 0x8664) { throw "$Pkg.exe is for machine 0x$('{0:X}' -f $machine), expected x64" }
if ($subsystem -ne 2) { throw "$Pkg.exe has PE subsystem $subsystem, expected 2 (Windows GUI)" }

$Stage = Join-Path $TargetDir 'windows-package\stage'
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item $Exe, $Cli $Stage
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md', 'THIRD-PARTY-LICENSES.txt') {
  Copy-Item -LiteralPath (Join-Path $Root $f) -Destination $Stage
}

$Msi = Join-Path $Dist "$Pkg-$Version-windows-x64.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot "$Pkg.wxs") -arch x64 `
    -d "Version=$MsiVersion" -d "BinDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\musiccraft.ico')" `
    -o $Msi
}
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))

$Portable = Join-Path $TargetDir "windows-package\$Pkg-$Version-windows-x64-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item $Exe, $Cli $Portable
foreach ($f in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md', 'THIRD-PARTY-LICENSES.txt') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
$Zip = Join-Path $Dist "$Pkg-$Version-windows-x64-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip
Get-Item $Msi, $Zip | Format-Table Name, Length
