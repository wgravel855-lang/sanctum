<#
  Sanctum verification.

  Runs everything that can be checked WITHOUT elevation (tests, integrity check,
  zero-telemetry proof, UI build), then prints the manual, elevated acceptance
  ritual. Run from the repository root:

      powershell -ExecutionPolicy Bypass -File tools\verify.ps1
#>

$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$fail = 0

function Section($name) { Write-Host "`n=== $name ===" -ForegroundColor Cyan }

Section "Rust tests"
cargo test --quiet
if ($LASTEXITCODE -ne 0) { Write-Host "FAIL: cargo test" -ForegroundColor Red; $fail++ }

Section "Blocklist integrity check"
$py = (Get-Command py -ErrorAction SilentlyContinue)
if ($null -eq $py) { $py = (Get-Command python -ErrorAction SilentlyContinue) }
if ($null -ne $py) {
    & $py.Source tools\integrity_check.py
    if ($LASTEXITCODE -ne 0) { Write-Host "FAIL: integrity check" -ForegroundColor Red; $fail++ }
} else {
    Write-Host "SKIP: no Python found" -ForegroundColor Yellow
}

Section "Zero-telemetry proof (each must find nothing)"
$patterns = @(
    @{ label = "analytics/telemetry SDKs"; terms = "analytics telemetry sentry mixpanel segment amplitude posthog gtag datadog bugsnag"; globs = @("crates\*.rs", "ui\src\*.ts", "ui\src\*.tsx") },
    @{ label = "HTTP-client crate dependency"; terms = "reqwest hyper ureq isahc";                                                        globs = @("Cargo.toml") },
    @{ label = "fetch/XHR in the UI";       terms = "XMLHttpRequest axios sendBeacon";                                                   globs = @("ui\src\*.ts", "ui\src\*.tsx") }
)
foreach ($p in $patterns) {
    $hits = findstr /S /I /M $p.terms $p.globs 2>$null
    if ($hits) {
        Write-Host ("FAIL: found " + $p.label + ":") -ForegroundColor Red
        $hits | ForEach-Object { Write-Host "   $_" }
        $fail++
    } else {
        Write-Host ("OK: no " + $p.label) -ForegroundColor Green
    }
}

Section "UI build"
if (Test-Path ui\node_modules) {
    Push-Location ui
    npm run build --silent
    if ($LASTEXITCODE -ne 0) { Write-Host "FAIL: ui build" -ForegroundColor Red; $fail++ }
    Pop-Location
} else {
    Write-Host "SKIP: run 'npm install' in ui\ first" -ForegroundColor Yellow
}

Section "Result"
if ($fail -eq 0) {
    Write-Host "All automated checks passed." -ForegroundColor Green
} else {
    Write-Host "$fail automated check(s) FAILED." -ForegroundColor Red
}

Write-Host @"

--- Manual acceptance ritual (run elevated, on a real machine) ---
Install:   .\target\release\sanctum-service.exe install
  1. Open a blocked domain in Chrome, Edge, Firefox, and an Incognito window -> all fail.
  2. google.com/search enforces SafeSearch; youtube.com is in Restricted Mode.
  3. Close the app -> still filtered. Kill sanctum-service.exe -> watchdog restarts it.
  4. Start a lock in the app, then confirm:
       - 'sanctum-service.exe uninstall' is refused
       - the timer can't be shortened and the block list can't shrink
       - the service 'Stop' button is greyed out in services.msc
  5. The lock screen names the Safe-Mode escape hatch.
  6. 'Delete all history' empties the activity log.
Remove (unlocked): .\target\release\sanctum-service.exe uninstall
Remove (locked):   reboot into Safe Mode, then .\target\release\sanctum-recover.exe
"@ -ForegroundColor Gray

exit $fail
