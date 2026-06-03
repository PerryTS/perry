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

cat >"$TMPDIR/array_property_index_set.ts" <<'TS'
interface State {
  width: number
  mirror: number[]
}

function write(state: State, cell: number, value: number): void {
  state.mirror[cell] = value
}

const state: State = { width: 20, mirror: new Array(40) }
write(state, 0, 112)
write(state, 1, 101)
state.mirror[2] = 114

if (state.mirror[0] !== 112 || state.mirror[1] !== 101 || state.mirror[2] !== 114) {
  throw new Error(
    `expected property array indexed writes to round-trip, got ${state.mirror[0]}, ${state.mirror[1]}, ${state.mirror[2]}`,
  )
}

console.log("array property index set ok")
TS

"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/array_property_index_set.ts" \
    -o "$TMPDIR/array_property_index_set" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$TMPDIR/array_property_index_set" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "array property index set ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: array property index set"
