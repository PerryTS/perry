#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

SRC="$TMPDIR/error_subclass_arrow_field_init.ts"
BIN="$TMPDIR/error_subclass_arrow_field_init"

cat > "$SRC" <<'TS'
class MyError extends Error {
  items: string[] = [];

  add = (item: string) => {
    this.items = [...this.items, item];
  };

  constructor() {
    super();
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

const error = new MyError();
console.log(
  typeof error.add,
  Object.keys(error).filter((key) => key !== "name").sort().join(","),
  error instanceof MyError,
  error instanceof Error,
);
error.add("item");
console.log(JSON.stringify(error.items));
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
