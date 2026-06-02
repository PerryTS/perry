#!/usr/bin/env bash
# Tier 11 — windows_smoke
#
# Native Windows app smoke test: compile a tiny perry/ui fixture for the
# windows target, launch under PERRY_UI_TEST_MODE so it self-exits, assert
# a clean exit. The actual orchestration (Start-Process / WaitForExit /
# GUI-subsystem handling) lives in scripts/smoke_windows_app.ps1 because
# launching a Win32 GUI app from bash routes through the Windows console
# subsystem differently than from PowerShell.
#
# Gate=windows in the orchestrator, so this never runs on macOS / Linux.
# This bash entrypoint exists because the orchestrator itself is bash; on
# Windows it executes via Git Bash / MSYS / WSL, and `powershell.exe` is
# always reachable from any of those.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 11)"
LOG="$TIER_DIR/windows_smoke.log"
SUMMARY="$TIER_DIR/summary.json"

# Locate PowerShell. pwsh (cross-platform) preferred over powershell.exe
# (legacy Windows PowerShell 5.1) but both work.
PS=""
if command -v pwsh >/dev/null 2>&1; then
    PS="pwsh"
elif command -v powershell.exe >/dev/null 2>&1; then
    PS="powershell.exe"
elif command -v powershell >/dev/null 2>&1; then
    PS="powershell"
else
    sweep_tier_emit "$OUT" 11 "windows_smoke" "FAIL" 0 "neither pwsh nor powershell.exe found on PATH"
    exit 0
fi

start="$(date +%s)"
set +e
PERRY_TEST_SUMMARY_OUT="$SUMMARY" \
    "$PS" -ExecutionPolicy Bypass -File "$REPO_ROOT/scripts/smoke_windows_app.ps1" \
    > "$LOG" 2>&1
rc=$?
set -e
end="$(date +%s)"
dur="$((end - start))"

# .ps1 returns: 0=PASS, 1=FAIL (real regression), 2=precondition missing (SKIP)
case "$rc" in
    0)
        if [[ -f "$SUMMARY" ]]; then
            phase="$(sed -nE 's/.*"phase"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' "$SUMMARY")"
            sweep_tier_emit "$OUT" 11 "windows_smoke" "PASS" "$dur" "phase=$phase"
        else
            sweep_tier_emit "$OUT" 11 "windows_smoke" "PASS" "$dur" "(no summary written but script exited 0)"
        fi
        ;;
    2)
        reason="$(grep -m1 -E 'not found|missing' "$LOG" 2>/dev/null | head -1)"
        sweep_tier_emit "$OUT" 11 "windows_smoke" "SKIP" "$dur" "${reason:-precondition not met}"
        ;;
    *)
        msg="exit=$rc"
        if [[ -f "$SUMMARY" ]]; then
            phase="$(sed -nE 's/.*"phase"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' "$SUMMARY")"
            msg="phase=$phase exit=$rc"
        fi
        sweep_tier_emit "$OUT" 11 "windows_smoke" "FAIL" "$dur" "$msg"
        ;;
esac
