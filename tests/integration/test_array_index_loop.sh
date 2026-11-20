#!/bin/bash
# Test: arr[i] in for-loop inside function must return correct element (not always arr[0])
# Bug: in large programs with many module-level arrays, arr[i] ignores index
#
# Uses bloom/jump's test_array_bug.ts which reliably reproduces the bug.
# This test requires the bloom/jump project to be checked out at ../../bloom/jump

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BLOOM_JUMP="$SCRIPT_DIR/../../../../bloom/jump"
OUTPUT="/tmp/perry_test_array_index_loop"

if [ ! -d "$BLOOM_JUMP/node_modules/bloom" ]; then
  echo "SKIP: ../../bloom/jump/node_modules/bloom not found (run: cd ../../bloom/jump && npm install)"
  exit 0
fi

if [ ! -f "$BLOOM_JUMP/src/test_array_bug.ts" ]; then
  echo "SKIP: ../../bloom/jump/src/test_array_bug.ts not found"
  exit 0
fi

cd "$BLOOM_JUMP"

# Compile the test
perry compile src/test_array_bug.ts -o "$OUTPUT" >/dev/null 2>&1
if [ $? -ne 0 ]; then
  echo "FAIL: compile error"
  exit 1
fi

# Run — output should contain "ALL TESTS PASSED" or specific FAIL lines
RUN_OUTPUT=$("$OUTPUT" 2>&1)
rm -f "$OUTPUT"

if echo "$RUN_OUTPUT" | grep -q "ALL TESTS PASSED"; then
  echo "PASS"
  exit 0
else
  # Show the failure details
  echo "$RUN_OUTPUT" | grep -E "FAIL|active=|expected"
  exit 1
fi
