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
function protoOwn(name: string) {
  const proto = (globalThis as any)[name]?.prototype;
  if (!proto) return null;
  return {
    hasOwn: Object.prototype.hasOwnProperty.call(proto, "isPrototypeOf"),
    hasOwnStatic: Object.hasOwn(proto, "isPrototypeOf"),
    descriptorIsUndefined: Object.getOwnPropertyDescriptor(proto, "isPrototypeOf") === undefined,
    namesHas: Object.getOwnPropertyNames(proto).includes("isPrototypeOf"),
  };
}

const blob = new Blob(["hello"], { type: "text/plain" });
const file = new File(["hello"], "hello.txt", { type: "text/plain" });

console.log(JSON.stringify({
  prototypes: {
    Array: protoOwn("Array"),
    Date: protoOwn("Date"),
    RegExp: protoOwn("RegExp"),
    Promise: protoOwn("Promise"),
    Map: protoOwn("Map"),
    Set: protoOwn("Set"),
    Blob: protoOwn("Blob"),
    File: protoOwn("File"),
    Object: protoOwn("Object"),
  },
  blob: {
    toString: Object.prototype.toString.call(blob),
    tag: (blob as any)[Symbol.toStringTag],
    ctorName: (blob as any).constructor?.name,
    hasCtor: "constructor" in (blob as any),
  },
  file: {
    toString: Object.prototype.toString.call(file),
    tag: (file as any)[Symbol.toStringTag],
    ctorName: (file as any).constructor?.name,
    hasCtor: "constructor" in (file as any),
  },
}));
TS

cat > "$TMPDIR/expected.log" <<'LOG'
{"prototypes":{"Array":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"Date":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"RegExp":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"Promise":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"Map":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"Set":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"Blob":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"File":{"hasOwn":false,"hasOwnStatic":false,"descriptorIsUndefined":true,"namesHas":false},"Object":{"hasOwn":true,"hasOwnStatic":true,"descriptorIsUndefined":false,"namesHas":true}},"blob":{"toString":"[object Blob]","tag":"Blob","ctorName":"Blob","hasCtor":true},"file":{"toString":"[object File]","tag":"File","ctorName":"File","hasCtor":true}}
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

echo "PASS: builtin prototype and fetch reflection"
