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

# The `in` operator walks the [[Prototype]] chain recursively. Two hazards:
#  1. A pathologically deep (non-cyclic) chain must not overflow the stack —
#     a missing key returns false (bounded like V8's own chain walk).
#  2. `Object.setPrototypeOf` must reject a cycle with a TypeError, exactly
#     like Node, so a cycle can never be built in the first place.
cat > "$TMPDIR/main.ts" <<'TS'
let deep: any = { bottomKey: 1 };
for (let i = 0; i < 3000; i++) {
  deep = Object.create(deep);
}
let cycleThrew = false;
try {
  const a: any = {};
  const b: any = {};
  Object.setPrototypeOf(a, b);
  Object.setPrototypeOf(b, a); // would form a 2-cycle -> must throw
} catch (e) {
  cycleThrew = e instanceof TypeError;
}
console.log(JSON.stringify({
  missingOnDeep: "nope" in deep,       // false, no stack overflow
  shallowPresent: "x" in Object.create({ x: 1 }), // true
  shallowMissing: "y" in Object.create({ x: 1 }),  // false
  cycleThrew,                          // true (matches Node)
}));
TS

if ! command -v node >/dev/null 2>&1; then
    echo "SKIP: node binary not found"
    exit 0
fi
node "$TMPDIR/main.ts" > "$TMPDIR/expected.log"

BIN="$TMPDIR/out"
COMPILE_ARGS=(compile --no-cache)
if [[ -f "$REPO_ROOT/target/debug/libperry_runtime.a" && -f "$REPO_ROOT/target/debug/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/debug"
    COMPILE_ARGS+=(--no-auto-optimize)
elif [[ -f "$REPO_ROOT/target/release/libperry_runtime.a" && -f "$REPO_ROOT/target/release/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/release"
    COMPILE_ARGS+=(--no-auto-optimize)
fi

"$PERRY" "${COMPILE_ARGS[@]}" "$TMPDIR/main.ts" -o "$BIN" > "$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$BIN" > "$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed (stack overflow on deep prototype chain?)"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/run.log"; then
    echo "FAIL: output mismatch vs node"
    exit 1
fi

echo "PASS: deep prototype chain has-property terminates and cycles throw"
