#!/bin/bash
# Regression for Undici's CJS `isomorphicDecode` path:
# `String.fromCharCode.apply(null, Uint8Array)` must lower without recursively
# re-entering the namespace-static `.apply` intrinsic and must materialize the
# typed-array bytes as call arguments.

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

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat > "$TMPDIR/main.js" << 'EOF'
'use strict'

const bytes = new Uint8Array([72, 101, 108, 108, 111])
const viaApply = String.fromCharCode.apply(null, bytes)
const viaSpread = String.fromCharCode(...bytes)
const min = Math.min.apply(null, new Uint8Array([3, 1, 2]))

if (viaApply !== 'Hello' || viaSpread !== 'Hello' || min !== 1) {
  console.log('bad:' + viaApply + ':' + viaSpread + ':' + min)
  process.exit(1)
}

console.log('ok:' + viaApply + ':' + min)
EOF

"$PERRY" compile --no-cache "$TMPDIR/main.js" -o "$TMPDIR/test_bin" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$TMPDIR/test_bin" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: runtime failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q '^ok:Hello:1$' "$TMPDIR/run.log"; then
    echo "FAIL: unexpected output"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: Undici String.fromCharCode.apply typed-array regression"
