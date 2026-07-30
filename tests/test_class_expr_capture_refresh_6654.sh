#!/bin/bash
# Regression (#6654): capture refreshes for class expressions must belong to
# the evaluated heap class object, including parameter defaults and module
# blocks. A template-name-keyed snapshot lets a later factory call overwrite an
# earlier class's environment; dropping/stranding refresh entries leaves later
# parameter/body assignments invisible.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
FIXTURE="$ROOT/test-parity/node-suite/object/class-expr-capture-refresh-edge.js"
PERRY="${PERRY:-$ROOT/target/release/perry}"
[ ! -f "$PERRY" ] && PERRY="$ROOT/target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi
if ! command -v node >/dev/null 2>&1; then
  echo "SKIP: node not available"
  exit 0
fi
if ! command -v cc >/dev/null 2>&1; then
  echo "SKIP: cc not available"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

NODE_OUTPUT=$(node "$FIXTURE")
COMPILE_OUTPUT=$(
  PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile "$FIXTURE" \
    -o "$TMPDIR/test_bin" --no-cache 2>&1
) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
}
PERRY_OUTPUT=$("$TMPDIR/test_bin")

if [ "$PERRY_OUTPUT" = "$NODE_OUTPUT" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: class-expression capture refresh diverged from Node"
echo "Node:"
echo "$NODE_OUTPUT"
echo "Perry:"
echo "$PERRY_OUTPUT"
exit 1
