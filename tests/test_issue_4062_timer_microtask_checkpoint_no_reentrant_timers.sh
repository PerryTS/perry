#!/bin/bash
# Regression for the OpenTUI/Perry native render abort: a timer callback that
# schedules another timer through a microtask checkpoint must not recursively
# process that timer while the first timer's exception trap is still active.

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

cat > "$TMPDIR/main.ts" << 'EOF'
let depth = 0;

async function step(): Promise<void> {
  depth++;
  console.log("timer:" + depth);
  if (depth < 180) {
    setTimeout(step, 0);
    await Promise.resolve(depth);
  }
}

setTimeout(step, 0);
setTimeout(() => {
  console.log("done:" + depth);
  process.exit(depth === 180 ? 0 : 1);
}, 20);
EOF

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1 \
    "$PERRY" compile --no-cache "$TMPDIR/main.ts" -o "$TMPDIR/test_bin" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

set +e
run_with_timeout 5 "$TMPDIR/test_bin" >"$TMPDIR/run.log" 2>&1
rc=$?
set -e

if [[ "$rc" -ne 0 ]]; then
    echo "FAIL: timer/microtask checkpoint fixture exited with $rc"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -120
    exit 1
fi

if ! grep -q '^done:180$' "$TMPDIR/run.log"; then
    echo "FAIL: timer chain did not complete"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -120
    exit 1
fi

echo "PASS"
