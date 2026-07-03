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

cat > "$TMPDIR/main.ts" <<'TS'
let count = 0;
const order: string[] = [];

function parent() {
  count++;
  return class Base {};
}

function make() {
  return class Child extends parent() {
    static before = order.push("before");
    static {
      order.push("block");
    }
    static after = order.push("after");
    static observed = count;
  };
}

const Child = make();
console.log(JSON.stringify({
  count,
  observed: (Child as any).observed,
  order,
  construct: new Child() instanceof Child,
}));
TS

cat > "$TMPDIR/expected.log" <<'LOG'
{"count":1,"observed":1,"order":["before","block","after"],"construct":true}
LOG

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

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/run.log"; then
    echo "FAIL: output mismatch"
    exit 1
fi

echo "PASS: dynamic class extends expression evaluated once"
