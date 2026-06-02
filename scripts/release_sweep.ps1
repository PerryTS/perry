# release_sweep.ps1 — Windows orchestrator wrapper.
#
# scripts/release_sweep.sh is bash. On Windows it requires Git Bash, MSYS,
# or WSL. This .ps1 detects bash on PATH and execs the bash orchestrator;
# if no bash is reachable, prints a clear message pointing the user at
# their options.
#
# Native PowerShell port of the orchestrator is intentionally out of scope:
# the bash orchestrator is the source of truth, the tier registry lives in
# release_sweep.sh, and a parallel PS implementation would drift. Git Bash
# is shipped with Git for Windows (which most Perry contributors install
# anyway for `git`), and WSL is a single-line winget install.
#
# Usage:
#   .\scripts\release_sweep.ps1                                    # run all
#   .\scripts\release_sweep.ps1 --tier=11                          # one tier
#   .\scripts\release_sweep.ps1 --gate-0.6.0 --allow-skip=8,9,10   # gate

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path "$PSScriptRoot\.."
$BashScript = Join-Path $RepoRoot "scripts\release_sweep.sh"

if (-not (Test-Path $BashScript)) {
    Write-Error "release_sweep.sh not found at $BashScript"
    exit 2
}

# Detect bash. Try in order: WSL bash, Git Bash, MSYS bash.
$BashExe = $null
foreach ($candidate in @("bash.exe", "C:\Program Files\Git\bin\bash.exe", "C:\Windows\System32\bash.exe")) {
    $resolved = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($resolved) {
        $BashExe = $resolved.Source
        break
    }
}

if (-not $BashExe) {
    Write-Host @"
release_sweep: no bash on PATH. Options:
  1. Install Git for Windows  — bundles Git Bash:  https://git-scm.com/download/win
  2. Enable WSL                — wsl --install
  3. Install MSYS2             — https://www.msys2.org/
"@
    exit 2
}

Write-Host "release_sweep.ps1: forwarding to $BashExe $BashScript $args"
& $BashExe $BashScript @args
exit $LASTEXITCODE
