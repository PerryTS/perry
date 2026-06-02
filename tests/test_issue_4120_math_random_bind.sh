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

cat >"$TMPDIR/issue_4120.ts" <<'TS'
const boundRandom = Math.random.bind(Math);

console.log(typeof boundRandom);
console.log(typeof boundRandom());

let threwTypeError = false;
try {
  Function.prototype.bind.call({});
} catch (error) {
  threwTypeError = error instanceof TypeError;
}

console.log(threwTypeError);
TS

"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/issue_4120.ts" -o "$TMPDIR/issue_4120" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

"$TMPDIR/issue_4120" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

EXPECTED=$'function\nnumber\ntrue'
ACTUAL="$(cat "$TMPDIR/run.log")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
    echo "FAIL: unexpected output"
    echo "expected:"
    printf '%s\n' "$EXPECTED" | sed 's/^/    /'
    echo "actual:"
    printf '%s\n' "$ACTUAL" | sed 's/^/    /'
    exit 1
fi

echo "PASS: Math.random.bind compatibility"
