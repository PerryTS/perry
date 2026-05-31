#!/bin/bash
# Regression for #3730: an awaited node:timers/promises timer scheduled with
# { ref: false } must not keep Perry's await loop alive forever.

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

UNREF_AWAIT_SRC="$REPO_ROOT/test-parity/node-suite/timers/promises/timeout-value-and-ref.ts"
UNREF_AWAIT_BIN="$TMPDIR/timeout-value-and-ref"

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1 \
    "$PERRY" compile --no-cache "$UNREF_AWAIT_SRC" -o "$UNREF_AWAIT_BIN" \
    >"$TMPDIR/compile_unref_await.log" 2>&1 || {
        echo "FAIL: compile failed for awaited ref:false timer"
        sed 's/^/    /' "$TMPDIR/compile_unref_await.log" | tail -80
        exit 1
    }

set +e
run_with_timeout 5 "$UNREF_AWAIT_BIN" >"$TMPDIR/run_unref_await.log" 2>&1
unref_await_rc=$?
set -e

if [[ "$unref_await_rc" -eq 124 ]]; then
    echo "FAIL: awaited ref:false timer hung"
    sed 's/^/    /' "$TMPDIR/run_unref_await.log" | tail -80
    exit 1
fi

if [[ "$unref_await_rc" -ne 13 ]]; then
    echo "FAIL: expected unsettled top-level await exit status 13, got $unref_await_rc"
    sed 's/^/    /' "$TMPDIR/run_unref_await.log" | tail -80
    exit 1
fi

if grep -q "value object:" "$TMPDIR/run_unref_await.log"; then
    echo "FAIL: ref:false timer unexpectedly resolved while it was the only pending work"
    sed 's/^/    /' "$TMPDIR/run_unref_await.log" | tail -80
    exit 1
fi

if ! grep -q "Detected unsettled top-level await" "$TMPDIR/run_unref_await.log"; then
    echo "FAIL: unsettled top-level await warning was not emitted"
    sed 's/^/    /' "$TMPDIR/run_unref_await.log" | tail -80
    exit 1
fi

FIRE_FORGET_SRC="$REPO_ROOT/test-parity/node-suite/timers/promises/ref-false-liveness.ts"
FIRE_FORGET_BIN="$TMPDIR/ref-false-liveness"

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1 \
    "$PERRY" compile --no-cache "$FIRE_FORGET_SRC" -o "$FIRE_FORGET_BIN" \
    >"$TMPDIR/compile_fire_forget.log" 2>&1 || {
        echo "FAIL: compile failed for fire-and-forget ref:false timer"
        sed 's/^/    /' "$TMPDIR/compile_fire_forget.log" | tail -80
        exit 1
    }

set +e
run_with_timeout 5 "$FIRE_FORGET_BIN" >"$TMPDIR/run_fire_forget.log" 2>&1
fire_forget_rc=$?
set -e

if [[ "$fire_forget_rc" -ne 0 ]]; then
    echo "FAIL: fire-and-forget ref:false liveness fixture exited with $fire_forget_rc"
    sed 's/^/    /' "$TMPDIR/run_fire_forget.log" | tail -80
    exit 1
fi

if [[ "$(cat "$TMPDIR/run_fire_forget.log")" != "scheduled" ]]; then
    echo "FAIL: fire-and-forget ref:false fixture output changed"
    sed 's/^/    /' "$TMPDIR/run_fire_forget.log" | tail -80
    exit 1
fi

echo "PASS"
