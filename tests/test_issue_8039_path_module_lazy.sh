#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
FIXTURE="$REPO_ROOT/test-files/issue_8039_path_modules"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: build Perry first or set PERRY_BIN"
    exit 0
fi

run_with_timeout() {
    local secs="$1"
    shift
    local rc
    if command -v timeout >/dev/null 2>&1; then
        if timeout --kill-after=1 "$secs" "$@"; then
            return 0
        else
            rc=$?
        fi
        [[ "$rc" == "124" || "$rc" == "143" || "$rc" == "137" ]] && return 124
        return "$rc"
    fi
    if command -v gtimeout >/dev/null 2>&1; then
        if gtimeout --kill-after=1 "$secs" "$@"; then
            return 0
        else
            rc=$?
        fi
        [[ "$rc" == "124" || "$rc" == "143" || "$rc" == "137" ]] && return 124
        return "$rc"
    fi
    # macOS has neither GNU timeout nor gtimeout by default. Avoid a shell
    # sleep/watchdog whose inherited stdout keeps command substitution open;
    # Python kills and reaps the child before returning 124.
    python3 - "$secs" "$@" <<'PY'
import subprocess
import os
import signal
import sys
import tempfile

def session_members(session_id):
    """Return every process still belonging to the isolated child session."""
    try:
        listing = subprocess.check_output(
            ["ps", "-axo", "pid="], text=True, stderr=subprocess.DEVNULL
        )
    except (OSError, subprocess.SubprocessError):
        return []
    members = []
    for line in listing.splitlines():
        try:
            pid = int(line)
            if os.getsid(pid) == session_id:
                members.append(pid)
        except (ProcessLookupError, PermissionError, ValueError):
            pass
    return members

timed_out = False
with tempfile.TemporaryFile() as output:
    process = subprocess.Popen(
        sys.argv[2:],
        start_new_session=True,
        stdout=output,
        stderr=subprocess.STDOUT,
    )
    try:
        rc = process.wait(timeout=float(sys.argv[1]))
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            pass
        # A descendant may create a separate process group, so kill every
        # process that remains in the isolated session before returning.
        for pid in session_members(process.pid):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        rc = process.wait()
    output.seek(0)
    sys.stdout.buffer.write(output.read())
if timed_out:
    raise SystemExit(124)
if rc in (-15, -9, 143, 137):
    raise SystemExit(124)
raise SystemExit(rc)
PY
}

(
    cd "$FIXTURE"
    "$PERRY" compile --no-auto-optimize entry.js -o "$WORK/path-module-lazy"
)

if ! OUTPUT="$(cd "$FIXTURE" && run_with_timeout 30 "$WORK/path-module-lazy" 2>&1)"; then
    echo "FAIL: path-module lazy binary timed out or exited non-zero"
    echo "$OUTPUT"
    exit 1
fi
EXPECTED="PASS: issue 8039 cold/warm path modules"
if [[ "$OUTPUT" != *"$EXPECTED"* ]]; then
    echo "FAIL: path-module lazy graph did not pass"
    echo "$OUTPUT"
    exit 1
fi

echo "$EXPECTED"
