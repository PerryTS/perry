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
export abstract class Class {
  constructor(..._args: any[]) {}
}

function makeCtor(name: string, initFn: (inst: any, def: any) => void) {
  function _(this: any, def: any) {
    initFn(this, def);
    return this;
  }
  Object.defineProperty(_, "name", { value: name });
  return _ as any;
}

const Real = makeCtor("Real", (inst, def) => {
  inst.tag = def.tag;
});

function factory(Class: any) {
  return new Class({ tag: "ok" });
}

const value = factory(Real);
console.log(JSON.stringify({
  proto: Object.getPrototypeOf(value)?.constructor?.name,
  tag: value.tag,
  inst: value instanceof (Real as any),
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
{"proto":"Real","tag":"ok","inst":true}
EOF_EXPECTED

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/run.log"; then
    echo "FAIL: output mismatch"
    exit 1
fi

echo "PASS: new parameter shadows same-named class"
