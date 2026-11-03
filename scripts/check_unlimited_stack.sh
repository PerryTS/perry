#!/usr/bin/env bash
# USG: the stack-overflow guard must work under every stack limit. Compiles the
# #10812 recursion gap test, runs it under the default limit and under
# `ulimit -s unlimited`, and requires the same output (RangeError caught, the
# program keeps running) and exit 0 from both.
# usage: scripts/check_unlimited_stack.sh <perry-binary>   (PERRY_RUNTIME_DIR as for any compile)
set -u
perry=${1:?usage: check_unlimited_stack.sh <perry-binary>}
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$perry" compile "$root/test-files/test_gap_10812_stack_overflow_range_error.ts" -o "$work/rec" >/dev/null 2>&1 || { echo "FAIL: compile"; exit 1; }
status=0
for limit in 8192 unlimited; do
  (ulimit -v 8000000; ulimit -s "$limit"; timeout 120 "$work/rec" >"$work/out-$limit" 2>&1)
  rc=$?
  if [ "$rc" != 0 ] || ! grep -q '^function: true Maximum call stack size exceeded$' "$work/out-$limit"; then
    echo "FAIL: ulimit -s $limit rc=$rc"; status=1
  else
    echo "ok: ulimit -s $limit"
  fi
done
cmp -s "$work/out-8192" "$work/out-unlimited" || { echo "FAIL: output differs between limits"; status=1; }
exit $status
