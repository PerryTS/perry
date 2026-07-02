#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/main.ts" <<'TS'
const formatted: any = { _errors: [] };
formatted.b = { _errors: [] };
formatted.b[0] = { _errors: ["x"] };
console.log(JSON.stringify({ formatted }, (_key, value) => value));
TS

BIN="$TMPDIR/main"
COMPILE_OUT="$($PERRY compile --no-cache --no-auto-optimize "$TMPDIR/main.ts" -o "$BIN" 2>&1)" || {
    echo "FAIL: perry compile errored"
    echo "$COMPILE_OUT"
    exit 1
}
OUT="$($BIN 2>&1)" || {
    echo "FAIL: compiled program errored"
    echo "$OUT"
    exit 1
}
EXPECTED='{"formatted":{"_errors":[],"b":{"0":{"_errors":["x"]},"_errors":[]}}}'
if [[ "$OUT" != "$EXPECTED" ]]; then
    echo "FAIL: unexpected output"
    echo "expected: $EXPECTED"
    echo "actual:   $OUT"
    exit 1
fi

echo "PASS: JSON.stringify replacer key order"
