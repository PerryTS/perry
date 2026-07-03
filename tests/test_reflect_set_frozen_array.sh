#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/main.ts" <<'TS'
const array = Object.freeze(["a"]);
const tupleLike = Object.freeze(["x"] as [string]);
const object = Object.freeze({ id: 1 });

function check(value: boolean, label: string) {
  if (!value) {
    throw new Error(label);
  }
}

check(Reflect.set(array, "0", "b") === false, "frozen array index Reflect.set result");
check(array[0] === "a", "frozen array index unchanged");
check(Reflect.set(tupleLike, "0", "y") === false, "frozen tuple-like index Reflect.set result");
check(tupleLike[0] === "x", "frozen tuple-like index unchanged");
check(Reflect.set(object, "id", 2) === false, "frozen object property Reflect.set result");
check(object.id === 1, "frozen object property unchanged");
console.log("OK");
TS

BIN="$TMPDIR/out"
COMPILE_ARGS=(compile --no-cache)
PERRY_DIR="$(cd "$(dirname "$PERRY")" && pwd)"
if [[ -f "$PERRY_DIR/libperry_runtime.a" && -f "$PERRY_DIR/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$PERRY_DIR"
    COMPILE_ARGS+=(--no-auto-optimize)
fi

"$PERRY" "${COMPILE_ARGS[@]}" "$TMPDIR/main.ts" -o "$BIN" > "$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$BIN" > "$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}
if ! grep -q "^OK$" "$TMPDIR/run.log"; then
    echo "FAIL: expected OK, got:"
    cat "$TMPDIR/run.log"
    exit 1
fi
echo "PASS: Reflect.set reports false for frozen array indices"
