#!/bin/bash
# Regression for #5056: passing an AbortSignal to events.once / events.on must
# not leave a leaked keepalive (pending promise / abort listener) parking the
# event loop. The programs finish their work, print the right output, and Node
# exits 0 — Perry must do the same instead of hanging in the event-loop park.
#
# The node-suite parity harness compares stdout but a hang only shows up as a
# timeout/exit-code mismatch, and the differential runner is not part of the
# per-PR gate, so this stand-alone guard pins the exit behaviour directly (same
# shape as tests/test_issue_3730_timers_promises_unref_await.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY:-$REPO_ROOT/target/release/perry}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
fi

run_with_timeout() {
    local secs="$1"
    shift
    if command -v timeout >/dev/null 2>&1; then
        timeout "$secs" "$@"
        return $?
    fi
    if command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$secs" "$@"
        return $?
    fi
    "$@" &
    local pid=$!
    ( sleep "$secs" && kill -TERM "$pid" 2>/dev/null && sleep 1 && kill -KILL "$pid" 2>/dev/null ) &
    local watcher=$!
    if wait "$pid" 2>/dev/null; then
        kill -TERM "$watcher" 2>/dev/null || true
        wait "$watcher" 2>/dev/null || true
        return 0
    fi
    local rc=$?
    kill -TERM "$watcher" 2>/dev/null || true
    wait "$watcher" 2>/dev/null || true
    [[ "$rc" == "143" ]] && return 124
    return "$rc"
}

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# fixture-name | source path | expected stdout
check_case() {
    local name="$1"
    local src="$2"
    local expected="$3"
    local bin="$TMPDIR/$name"

    env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1 \
        "$PERRY" compile --no-cache "$src" -o "$bin" \
        >"$TMPDIR/compile_$name.log" 2>&1 || {
            echo "FAIL: compile failed for $name"
            sed 's/^/    /' "$TMPDIR/compile_$name.log" | tail -80
            exit 1
        }

    set +e
    run_with_timeout 8 "$bin" >"$TMPDIR/run_$name.log" 2>&1
    local rc=$?
    set -e

    if [[ "$rc" -eq 124 ]]; then
        echo "FAIL: $name hung at exit (event loop parked on a leaked keepalive)"
        sed 's/^/    /' "$TMPDIR/run_$name.log" | tail -80
        exit 1
    fi
    if [[ "$rc" -ne 0 ]]; then
        echo "FAIL: $name exited with $rc (expected 0)"
        sed 's/^/    /' "$TMPDIR/run_$name.log" | tail -80
        exit 1
    fi
    if [[ "$(cat "$TMPDIR/run_$name.log")" != "$expected" ]]; then
        echo "FAIL: $name output mismatch"
        echo "  expected:"; printf '%s\n' "$expected" | sed 's/^/    /'
        echo "  got:"; sed 's/^/    /' "$TMPDIR/run_$name.log"
        exit 1
    fi
    echo "  ok: $name"
}

check_case "once-abort" \
    "$REPO_ROOT/test-parity/node-suite/events/once/abort-cleans-pending-error.ts" \
    "abort: AbortError:ABORT_ERR
late error: late"

check_case "on-abort" \
    "$REPO_ROOT/test-parity/node-suite/events/on/async-iterator-abort.ts" \
    "abort: AbortError ABORT_ERR
seen: a,b"

echo "PASS"
