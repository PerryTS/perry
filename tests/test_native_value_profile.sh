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

EXPECTED="size=24,align=8,sequence=8,length=1,flags=4294967295,sequenceValue=9007199254740991,gainRounded=true,tiny=2:1:255:255:7,narrow=16:2:4:8:-128:65535:-32768:-9007199254740991:-5:65535:-1024:-42,header=7:42:true,rejectedFraction=true,rejectedType=true,rejectedOctet=true,rejectedSignedByte=true,rejectedHalfWord=true,rejectedSignedHalfWord=true,rejectedSignedSize=true"
ACTUAL=$(./native_value_profile)
if [ "$ACTUAL" != "$EXPECTED" ]; then
  echo "FAIL: perry/native output mismatch"
  echo "Expected: $EXPECTED"
  echo "Actual:   $ACTUAL"
  exit 1
fi

echo "PASS"
