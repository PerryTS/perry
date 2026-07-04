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
function makeCtor(name: string, Parent?: any) {
  const Base = Parent ?? Object;
  class Definition extends Base {}
  function _(this: any) {
    const inst = Parent ? new Definition() : this;
    inst.name = name;
    return inst;
  }
  Object.defineProperty(_, "name", { value: name });
  Object.defineProperty(_, "prototype", { value: Definition.prototype });
  Object.defineProperty(Definition.prototype, "constructor", { value: _ });
  return _;
}
const Real: any = makeCtor("Real", Error);
makeCtor("Other");
const error = new Real();
const proto = Object.getPrototypeOf(error);
const parentProto = proto && Object.getPrototypeOf(proto);
console.log(JSON.stringify({
  error: error instanceof Error,
  real: error instanceof Real,
  // Pin the exact chain depth: error -> Real.prototype -> Error.prototype ->
  // Object.prototype. A disjunctive `parentProto === Error.prototype || proto
  // === Error.prototype` would accept a flattened (downgraded) chain too.
  protoIsRealProto: proto === Real.prototype,
  parentIsErrorProto: parentProto === Error.prototype,
  protoNotErrorDirect: proto !== Error.prototype,
  grandIsObjectProto: Object.getPrototypeOf(parentProto) === Object.prototype,
}));
TS

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
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

cat > "$TMPDIR/expected.log" <<'EOF_EXPECTED'
{"error":true,"real":true,"protoIsRealProto":true,"parentIsErrorProto":true,"protoNotErrorDirect":true,"grandIsObjectProto":true}
EOF_EXPECTED

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/run.log"; then
    echo "FAIL: output mismatch"
    exit 1
fi

echo "PASS: dynamic builtin parent no downgrade"
