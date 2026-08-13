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
