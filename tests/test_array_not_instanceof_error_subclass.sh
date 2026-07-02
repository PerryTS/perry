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

cat >"$TMPDIR/main.ts" <<'TS'
class ArraySubclass extends Array {}
class MapSubclass extends Map {}
class SetSubclass extends Set {}
class CustomA extends Error {}
class CustomB extends Error {}

const arrays = [
  [],
  [{}],
  [{ code: "invalid_format", path: [], message: "Invalid URL", format: "url" }],
  new Array(1),
  new Array(2),
  [new CustomA("x")],
];

for (let i = 0; i < arrays.length; i++) {
  console.log(i, arrays[i] instanceof Error, arrays[i] instanceof CustomA, arrays[i] instanceof CustomB);
}

const values = [
  ["array", new ArraySubclass(1, 2)],
  ["map", new MapSubclass([["a", 1]])],
  ["set", new SetSubclass([1])],
  ["error", new CustomA("boom")],
] as const;

for (const [name, value] of values) {
  console.log(name, JSON.stringify({
    ArraySubclass: value instanceof ArraySubclass,
    Array: value instanceof Array,
    MapSubclass: value instanceof MapSubclass,
    Map: value instanceof Map,
    SetSubclass: value instanceof SetSubclass,
    Set: value instanceof Set,
    CustomA: value instanceof CustomA,
    Error: value instanceof Error,
    Object: value instanceof Object,
  }));
}
TS

cat >"$TMPDIR/expected.log" <<'EOF_EXPECTED'
0 false false false
1 false false false
2 false false false
3 false false false
4 false false false
5 false false false
array {"ArraySubclass":true,"Array":true,"MapSubclass":false,"Map":false,"SetSubclass":false,"Set":false,"CustomA":false,"Error":false,"Object":true}
map {"ArraySubclass":false,"Array":false,"MapSubclass":true,"Map":true,"SetSubclass":false,"Set":false,"CustomA":false,"Error":false,"Object":true}
set {"ArraySubclass":false,"Array":false,"MapSubclass":false,"Map":false,"SetSubclass":true,"Set":true,"CustomA":false,"Error":false,"Object":true}
error {"ArraySubclass":false,"Array":false,"MapSubclass":false,"Map":false,"SetSubclass":false,"Set":false,"CustomA":true,"Error":true,"Object":true}
EOF_EXPECTED

COMPILE_ARGS=(compile --no-cache)
if [[ -f "$REPO_ROOT/target/debug/libperry_runtime.a" && -f "$REPO_ROOT/target/debug/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/debug"
    COMPILE_ARGS+=(--no-auto-optimize)
elif [[ -f "$REPO_ROOT/target/release/libperry_runtime.a" && -f "$REPO_ROOT/target/release/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/release"
    COMPILE_ARGS+=(--no-auto-optimize)
fi

COMPILE_OUT="$(PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY" "${COMPILE_ARGS[@]}" "$TMPDIR/main.ts" -o "$TMPDIR/out" 2>&1)" || {
    echo "FAIL: perry compile errored"
    echo "$COMPILE_OUT"
    exit 1
}

"$TMPDIR/out" >"$TMPDIR/stdout.log" 2>"$TMPDIR/stderr.log" || {
    echo "FAIL: compiled binary errored"
    sed 's/^/    /' "$TMPDIR/stdout.log"
    sed 's/^/    /' "$TMPDIR/stderr.log"
    exit 1
}

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/stdout.log"; then
    echo "FAIL: stdout mismatch"
    exit 1
fi

if [[ -s "$TMPDIR/stderr.log" ]]; then
    echo "FAIL: expected empty stderr"
    sed 's/^/    /' "$TMPDIR/stderr.log"
    exit 1
fi

echo "PASS: arrays are not Error subclass instances"
