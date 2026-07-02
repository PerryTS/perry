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

cat > "$TMPDIR/main.js" <<'JS'
function run(label, fn) {
  try {
    console.log(label, JSON.stringify(fn()));
  } catch (e) {
    console.log(label, e.name + ":" + e.message);
  }
}

run("objectToString", () => ({}).toString());
run("objectToLocaleString", () => ({}).toLocaleString());
run("objectValueOfObject", () => ({}).valueOf() instanceof Object);
run("arrayToString", () => [1, 2].toString());
run("mapToString", () => new Map().toString());
run("mapToLocaleString", () => new Map().toLocaleString());
run("mapValueOfSelf", () => {
  const m = new Map();
  return m.valueOf() === m;
});
run("setToString", () => new Set().toString());
run("setToLocaleString", () => new Set().toLocaleString());
run("setValueOfSelf", () => {
  const s = new Set();
  return s.valueOf() === s;
});
run("ownToString", () => ({ toString() { return "own"; } }).toString());
run("classToString", () => new (class { toString() { return "class"; } })().toString());
JS

BIN="$TMPDIR/out"
COMPILE_ARGS=(compile --no-cache)
if [[ -f "$REPO_ROOT/target/debug/libperry_runtime.a" && -f "$REPO_ROOT/target/debug/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/debug"
    COMPILE_ARGS+=(--no-auto-optimize)
elif [[ -f "$REPO_ROOT/target/release/libperry_runtime.a" && -f "$REPO_ROOT/target/release/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/release"
    COMPILE_ARGS+=(--no-auto-optimize)
fi

"$PERRY" "${COMPILE_ARGS[@]}" "$TMPDIR/main.js" -o "$BIN" > "$TMPDIR/compile.log" 2>&1 || {
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
objectToString "[object Object]"
objectToLocaleString "[object Object]"
objectValueOfObject true
arrayToString "1,2"
mapToString "[object Map]"
mapToLocaleString "[object Map]"
mapValueOfSelf true
setToString "[object Set]"
setToLocaleString "[object Set]"
setValueOfSelf true
ownToString "own"
classToString "class"
EOF_EXPECTED

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/run.log"; then
    echo "FAIL: output mismatch"
    exit 1
fi

echo "PASS: Object.prototype method call fallback"
