#!/bin/bash
# Regression (fix/headers-iterator-segfault): js_get_iterator /
# is_builtin_iterator_class_id must not dereference a Web-Fetch handle
# (Headers / Request.headers / Response.headers) as a heap object.
#
# A `new Headers()` is a NaN-boxed small handle id in [0x40000, 0x100000), not
# a heap pointer. #4786 switched the `for...of` lowering for opaque iterables
# from the eager `js_for_of_to_array` path to the lazy iterator protocol
# (`GetIterator(obj)` -> `js_get_iterator`). `js_get_iterator` calls
# `is_builtin_iterator_class_id` to short-circuit values that are already
# iterators; pre-fix that helper floored pointer validation at 0x1008, so the
# 0x40000+ Headers handle slipped through and the runtime dereferenced
# `[handle - GC_HEADER_SIZE]` as a GcHeader -> SIGSEGV. This crashed every Hono
# HTTP response (`buildOutgoingHttpHeaders` iterates the response Headers) on
# Linux x86_64. macOS arm64 masked it via a higher heap floor.
#
# Expected: prints PASS and exits 0. Pre-fix: SIGSEGV (exit 139), no "PASS".
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC="$SCRIPT_DIR/test_headers_iterator_handle_segfault.ts"
OUTPUT="/tmp/perry_test_headers_iterator_handle_segfault"
PERRY="${PERRY:-perry}"

if ! command -v "$PERRY" >/dev/null 2>&1 && [ ! -x "$PERRY" ]; then
  echo "SKIP: perry binary not found (set PERRY=/path/to/perry)"
  exit 0
fi

"$PERRY" compile "$SRC" -o "$OUTPUT" >/tmp/perry_test_hi_compile.log 2>&1
if [ $? -ne 0 ]; then
  echo "FAIL: compile error"
  tail -20 /tmp/perry_test_hi_compile.log
  exit 1
fi

RUN_OUTPUT="$("$OUTPUT" 2>&1)"
RUN_EXIT=$?
rm -f "$OUTPUT"

if [ $RUN_EXIT -ne 0 ]; then
  echo "FAIL: runtime exited $RUN_EXIT (139 = SIGSEGV = the regression)"
  echo "$RUN_OUTPUT"
  exit 1
fi

if echo "$RUN_OUTPUT" | grep -q "PASS"; then
  echo "PASS"
  exit 0
else
  echo "FAIL: unexpected output"
  echo "$RUN_OUTPUT"
  exit 1
fi
