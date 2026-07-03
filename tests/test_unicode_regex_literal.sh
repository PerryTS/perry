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
if ! command -v node >/dev/null 2>&1; then
    echo "SKIP: node binary not found"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat > "$TMPDIR/main.ts" <<'TS'
const composed = "CAFE\u0301!".toLowerCase().normalize("NFC");
const literal = /^café!$/;
const constructor = new RegExp("^café!$");

console.log(JSON.stringify({
  direct: literal.test("café!"),
  composed,
  composedCodepoints: [...composed].map((char) => char.codePointAt(0)),
  literalSource: literal.source,
  literalMatch: literal.test(composed),
  constructorSource: constructor.source,
  constructorMatch: constructor.test(composed),
}));
TS

node "$TMPDIR/main.ts" > "$TMPDIR/expected.log"

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

echo "PASS: Unicode regex literal"
