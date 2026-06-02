# smoke_windows_app.ps1 — Windows-host smoke for tier 11 of release_sweep.sh.
#
# Compiles a tiny perry/ui app for the windows target via `perry compile
# --target windows`, launches it under PERRY_UI_TEST_MODE so it self-exits
# after one frame, asserts a clean exit. Invoked from
# scripts/release_sweep_tiers/tier11_windows_smoke.sh.
#
# Why a separate .ps1 rather than inlining in tier11_windows_smoke.sh: the
# release-sweep orchestrator is bash and tier scripts are bash, but native
# Windows process orchestration (Start-Process, Wait-Process, the GUI subsystem
# of the produced .exe) is PowerShell-shaped — not least because launching a
# Win32 GUI app from Git Bash routes through the Windows console subsystem
# and hangs differently than from a native console.
#
# Env (read from $env: namespace):
#   PERRY_BIN              path to perry.exe (default: target\release\perry.exe)
#   PERRY_TEST_SUMMARY_OUT release_sweep.sh hook
#   LAUNCH_TIMEOUT_SECS    per-launch deadline (default: 30)
#
# Exit codes:
#   0 — fixture compiled and self-exited cleanly
#   1 — perry compile or launch failed; details in stderr
#   2 — preconditions not met (perry.exe missing, etc.) — release_sweep
#       maps this to SKIP

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path "$PSScriptRoot\.."
$Fixture = Join-Path $RepoRoot "tests\release\link_smoke\fixture.ts"
$PerryBin = if ($env:PERRY_BIN) { $env:PERRY_BIN } else { Join-Path $RepoRoot "target\release\perry.exe" }
$LaunchTimeoutSecs = if ($env:LAUNCH_TIMEOUT_SECS) { [int]$env:LAUNCH_TIMEOUT_SECS } else { 30 }

if (-not (Test-Path $PerryBin)) {
    Write-Error "perry binary not found at $PerryBin (build with cargo build --release -p perry first)"
    exit 2
}
if (-not (Test-Path $Fixture)) {
    Write-Error "fixture not found at $Fixture"
    exit 2
}

$OutDir = Join-Path $RepoRoot "target\perry-windows-smoke"
New-Item -Force -ItemType Directory $OutDir | Out-Null
$ArtifactExe = Join-Path $OutDir "smoke.exe"

if (Test-Path $ArtifactExe) { Remove-Item $ArtifactExe }

# Step 1: compile
Write-Host "[1/3] perry compile --target windows ..."
$compileLog = Join-Path $OutDir "compile.log"
$proc = Start-Process -FilePath $PerryBin `
    -ArgumentList "compile", "--target", "windows", $Fixture, "-o", $ArtifactExe `
    -NoNewWindow -PassThru -Wait `
    -RedirectStandardOutput $compileLog -RedirectStandardError "$compileLog.err"

if ($proc.ExitCode -ne 0) {
    Write-Host "FAIL — perry compile exited $($proc.ExitCode)"
    Get-Content $compileLog -Tail 20 | ForEach-Object { Write-Host "  $_" }
    if (Test-Path "$compileLog.err") {
        Get-Content "$compileLog.err" -Tail 20 | ForEach-Object { Write-Host "  $_" }
    }
    if ($env:PERRY_TEST_SUMMARY_OUT) {
        Set-Content -Path $env:PERRY_TEST_SUMMARY_OUT -Value "{`"script`": `"smoke_windows_app.ps1`", `"passed`": 0, `"failed`": 1, `"skipped`": 0, `"phase`": `"compile`"}"
    }
    exit 1
}

if (-not (Test-Path $ArtifactExe)) {
    Write-Host "FAIL — perry compile exited 0 but produced no $ArtifactExe"
    if ($env:PERRY_TEST_SUMMARY_OUT) {
        Set-Content -Path $env:PERRY_TEST_SUMMARY_OUT -Value "{`"script`": `"smoke_windows_app.ps1`", `"passed`": 0, `"failed`": 1, `"skipped`": 0, `"phase`": `"compile-no-artifact`"}"
    }
    exit 1
}

# Step 2: launch with PERRY_UI_TEST_MODE
Write-Host "[2/3] launch with PERRY_UI_TEST_MODE=1 ..."
$env:PERRY_UI_TEST_MODE = "1"
$env:PERRY_UI_TEST_EXIT_AFTER_MS = "500"

$runLog = Join-Path $OutDir "run.log"
$launchProc = Start-Process -FilePath $ArtifactExe `
    -NoNewWindow -PassThru `
    -RedirectStandardOutput $runLog -RedirectStandardError "$runLog.err"

if (-not $launchProc.WaitForExit($LaunchTimeoutSecs * 1000)) {
    Write-Host "FAIL — process did not exit within ${LaunchTimeoutSecs}s; killing"
    $launchProc | Stop-Process -Force
    if ($env:PERRY_TEST_SUMMARY_OUT) {
        Set-Content -Path $env:PERRY_TEST_SUMMARY_OUT -Value "{`"script`": `"smoke_windows_app.ps1`", `"passed`": 0, `"failed`": 1, `"skipped`": 0, `"phase`": `"launch-timeout`"}"
    }
    exit 1
}

# Step 3: assert clean exit
Write-Host "[3/3] verify exit code ..."
if ($launchProc.ExitCode -ne 0) {
    Write-Host "FAIL — process exited $($launchProc.ExitCode)"
    Get-Content $runLog -Tail 20 -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "  $_" }
    if ($env:PERRY_TEST_SUMMARY_OUT) {
        Set-Content -Path $env:PERRY_TEST_SUMMARY_OUT -Value "{`"script`": `"smoke_windows_app.ps1`", `"passed`": 0, `"failed`": 1, `"skipped`": 0, `"phase`": `"launch-nonzero-exit`"}"
    }
    exit 1
}

Write-Host "PASS — windows fixture compiled, launched, and exited cleanly"
if ($env:PERRY_TEST_SUMMARY_OUT) {
    Set-Content -Path $env:PERRY_TEST_SUMMARY_OUT -Value "{`"script`": `"smoke_windows_app.ps1`", `"passed`": 1, `"failed`": 0, `"skipped`": 0, `"phase`": `"ok`"}"
}
exit 0
