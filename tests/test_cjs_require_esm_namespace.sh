#!/usr/bin/env bash
# Regression: CJS require() of a native ESM module with only named exports
# must import the ESM namespace, not a nonexistent default binding.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
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

cat >"$TMPDIR/esm.mjs" <<'JS'
export const value = 41
export function inc(x) {
  return x + 1
}
JS

cat >"$TMPDIR/cjs.js" <<'JS'
const esm = require("./esm.mjs")
console.log(esm.inc(esm.value))
JS

set +e
env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-auto-optimize \
    "$TMPDIR/cjs.js" -o "$TMPDIR/cjs-require-esm" \
    >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -ne 0 ]]; then
    echo "FAIL: CJS require of ESM namespace failed to compile"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

output="$("$TMPDIR/cjs-require-esm")"
if [[ "$output" != "42" ]]; then
    echo "FAIL: expected 42, got: $output"
    exit 1
fi

echo "PASS"
