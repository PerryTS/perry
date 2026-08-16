#!/bin/bash
# End-to-end regression for issue #6827's first public native-value slice:
# `perry/native` imports must resolve without a runtime module and lower to the
# existing verifier-backed POD layout and NativeArena intrinsics.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."
TEST_TMPDIR=$(mktemp -d)
trap 'rm -rf "$TEST_TMPDIR"' EXIT

if [ -z "${PERRY:-}" ]; then
  cargo build -q -p perry
  PERRY="$REPO_ROOT/target/debug/perry"
fi

case "$PERRY" in
  /*) ;;
  *) PERRY="$(pwd)/$PERRY" ;;
esac

if [ ! -x "$PERRY" ]; then
  echo "FAIL: perry binary not found at $PERRY"
  exit 1
fi

cp "$SCRIPT_DIR/fixtures/native_value_profile.ts" "$TEST_TMPDIR/main.ts"

cd "$TEST_TMPDIR"
"$PERRY" compile main.ts --output native_value_profile --no-cache >/dev/null

EXPECTED="size=16,align=8,sequence=8,length=1"
ACTUAL=$(./native_value_profile)
if [ "$ACTUAL" != "$EXPECTED" ]; then
  echo "FAIL: perry/native output mismatch"
  echo "Expected: $EXPECTED"
  echo "Actual:   $ACTUAL"
  exit 1
fi

echo "PASS"
