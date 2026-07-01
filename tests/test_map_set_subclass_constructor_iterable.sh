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
class FixtureMap extends Map<string, { count: number }> {}
class FixtureSet extends Set<{ id: number }> {}
class ChildMap extends FixtureMap {}
class ChildSet extends FixtureSet {}

const mapInput = new FixtureMap([["a", { count: 1 }]]);
const copiedMap = new Map(mapInput);
const childMap = new ChildMap([["b", { count: 2 }]]);
const setValue = { id: 1 };
const setInput = new FixtureSet([setValue]);
const copiedSet = new Set(setInput);
const childSetValue = { id: 2 };
const childSet = new ChildSet([childSetValue]);

console.log(JSON.stringify({
  mapInputIsMap: mapInput instanceof Map,
  mapInputIsSubclass: mapInput instanceof FixtureMap,
  mapInputSize: mapInput.size,
  mapInputGet: mapInput.get("a"),
  mapInputEntries: Array.from(mapInput.entries()),
  copiedMapSize: copiedMap.size,
  copiedMapGet: copiedMap.get("a"),
  copiedMapEntries: Array.from(copiedMap.entries()),
  childMapIsSubclass: childMap instanceof ChildMap,
  childMapEntries: Array.from(childMap.entries()),
  setInputIsSet: setInput instanceof Set,
  setInputIsSubclass: setInput instanceof FixtureSet,
  setInputSize: setInput.size,
  setInputValues: Array.from(setInput.values()),
  copiedSetSize: copiedSet.size,
  copiedSetValues: Array.from(copiedSet.values()),
  copiedSetHasOriginal: copiedSet.has(setValue),
  childSetIsSubclass: childSet instanceof ChildSet,
  childSetValues: Array.from(childSet.values()),
  childSetHasOriginal: childSet.has(childSetValue),
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
{"mapInputIsMap":true,"mapInputIsSubclass":true,"mapInputSize":1,"mapInputGet":{"count":1},"mapInputEntries":[["a",{"count":1}]],"copiedMapSize":1,"copiedMapGet":{"count":1},"copiedMapEntries":[["a",{"count":1}]],"childMapIsSubclass":true,"childMapEntries":[["b",{"count":2}]],"setInputIsSet":true,"setInputIsSubclass":true,"setInputSize":1,"setInputValues":[{"id":1}],"copiedSetSize":1,"copiedSetValues":[{"id":1}],"copiedSetHasOriginal":true,"childSetIsSubclass":true,"childSetValues":[{"id":2}],"childSetHasOriginal":true}
EOF_EXPECTED

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/run.log"; then
    echo "FAIL: output mismatch"
    exit 1
fi

echo "PASS: Map/Set subclass constructor iterable"
