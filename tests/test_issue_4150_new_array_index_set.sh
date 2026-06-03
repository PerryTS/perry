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

cat >"$TMPDIR/new_array_index_set.ts" <<'TS'
const values = new Array(4);
values[0] = 112;
values[1] = 101;

if (values.length !== 4) {
  throw new Error("expected preserved array length");
}
if (values[0] !== 112 || values[1] !== 101) {
  throw new Error(`expected indexed writes to round-trip, got ${values[0]}, ${values[1]}`);
}
if (values[2] !== undefined) {
  throw new Error("expected unwritten new Array slot to remain undefined");
}

console.log("new Array index set ok");
TS

"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/new_array_index_set.ts" \
    -o "$TMPDIR/new_array_index_set" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$TMPDIR/new_array_index_set" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "new Array index set ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: new Array index set"
