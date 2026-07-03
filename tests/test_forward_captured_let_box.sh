#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

SRC="$TMPDIR/forward_captured_let_box.ts"
BIN="$TMPDIR/forward_captured_let_box"

cat > "$SRC" <<'TS'
function makeGetter() {
  const getter = () => value;
  const value = { ok: true, nested: [1, "x"] };
  return getter;
}

const getter = makeGetter();
console.log(JSON.stringify(getter()));
TS

if ! command -v node >/dev/null 2>&1; then
    echo "SKIP: node binary not found"
    exit 0
fi
node "$SRC" > "$TMPDIR/expected.txt"

if [ -x "$ROOT/target/debug/perry" ]; then
  PERRY="$ROOT/target/debug/perry"
  export PERRY_RUNTIME_DIR="$ROOT/target/debug"
else
  PERRY="$ROOT/target/release/perry"
  export PERRY_RUNTIME_DIR="$ROOT/target/release"
fi

"$PERRY" compile --no-cache --no-auto-optimize "$SRC" -o "$BIN" > "$TMPDIR/compile.log" 2>&1 || {
  cat "$TMPDIR/compile.log"
  exit 1
}

"$BIN" > "$TMPDIR/actual.txt" 2> "$TMPDIR/run.log" || {
  cat "$TMPDIR/run.log"
  exit 1
}

diff -u "$TMPDIR/expected.txt" "$TMPDIR/actual.txt"
