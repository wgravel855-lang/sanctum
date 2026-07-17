<#
  Builds the Sanctum Windows installer (NSIS).

  It compiles the three service binaries in release, stages them where the
  Tauri bundler expects its `externalBin` sidecars (with the target-triple
  suffix), then builds the Tauri app + installer.

  Requirements: Rust (MSVC), Node, and the Tauri CLI (installed via `npm i`
  in ui/). Run from anywhere:

      powershell -ExecutionPolicy Bypass -File tools\build-installer.ps1

  Output: ui\src-tauri\target\release\bundle\nsis\Sanctum_<version>_x64-setup.exe
#>

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$triple = "x86_64-pc-windows-msvc"
$binaries = @("sanctum-service", "sanctum-watchdog", "sanctum-recover")
$stage = Join-Path $root "ui\src-tauri\binaries"

Write-Host "==> Building service binaries (release)..." -ForegroundColor Cyan
$binArgs = @()
foreach ($b in $binaries) { $binArgs += @("--bin", $b) }
cargo build --release --manifest-path (Join-Path $root "Cargo.toml") @binArgs
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Write-Host "==> Staging sidecars into ui\src-tauri\binaries\ ..." -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $stage | Out-Null
foreach ($b in $binaries) {
    $src = Join-Path $root "target\release\$b.exe"
    $dst = Join-Path $stage "$b-$triple.exe"
    Copy-Item $src $dst -Force
    Write-Host "    $b.exe -> $b-$triple.exe"
}

Write-Host "==> Building the Tauri app + NSIS installer..." -ForegroundColor Cyan
Push-Location (Join-Path $root "ui")
try {
    if (-not (Test-Path "node_modules")) { npm install }
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
} finally {
    Pop-Location
}

$out = Join-Path $root "ui\src-tauri\target\release\bundle\nsis"
Write-Host "`n==> Done. Installer(s) in:" -ForegroundColor Green
Write-Host "    $out"
Get-ChildItem $out -Filter *.exe -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "    $($_.Name)" }
