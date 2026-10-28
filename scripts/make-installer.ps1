# Builds the Windows installer: dist\Light-Studio-<version>-setup.exe
# Run in PowerShell from the repository root:  .\scripts\make-installer.ps1
# Needs Rust (rustup), Visual Studio Build Tools (C++) and Inno Setup 6 (https://jrsoftware.org/isdl.php).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"(.*)"' | Select-Object -First 1).Matches.Groups[1].Value

cargo build --release -p light-studio
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$iscc = @(
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
  "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
  "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 is not installed (https://jrsoftware.org/isdl.php)" }

New-Item -ItemType Directory -Force dist | Out-Null
& $iscc "/DAppVersion=$version" "installer\light-studio.iss"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }

Write-Host ""
Write-Host "Listo: dist\Light-Studio-$version-setup.exe"
