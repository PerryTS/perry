#!/bin/bash
# Regression (fix/object-field-read-segfault): js_object_get_own_field_or_undef
# must not dereference a Web-Fetch handle (Headers/Request/Response/Blob) as a
# heap object.
#
# A `new Headers()` is a NaN-boxed small handle id in [0x40000, 0x100000), not a
# heap pointer. When a user class declares a method whose name collides with a
# Headers method (`.set`/`.delete`/`.append`), the codegen dynamic-dispatch tower
# fires an own-property override probe (`js_object_get_own_field_or_undef`) on the
# receiver before dispatch. Pre-fix that probe floored pointer validation at
# 0x10000, so the 0x40000 Headers handle slipped through and the runtime
# dereferenced `[handle - GC_HEADER_SIZE]` as a GcHeader -> SIGSEGV. This crashed
# every Hono HTTP response (c.json -> #newResponse -> responseHeaders.set) on
# Linux x86_64. macOS arm64 masked it via is_valid_obj_ptr's higher heap floor.
#
# Expected: prints PASS and exits 0. Pre-fix: SIGSEGV (exit 139), no "PASS".
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC="$SCRIPT_DIR/test_headers_handle_field_probe_segfault.ts"
OUTPUT="/tmp/perry_test_headers_handle_field_probe_segfault"
PERRY="${PERRY:-perry}"

if ! command -v "$PERRY" >/dev/null 2>&1 && [ ! -x "$PERRY" ]; then
  echo "SKIP: perry binary not found (set PERRY=/path/to/perry)"
  exit 0
fi

"$PERRY" compile "$SRC" -o "$OUTPUT" >/tmp/perry_test_hh_compile.log 2>&1
if [ $? -ne 0 ]; then
  echo "FAIL: compile error"
  tail -20 /tmp/perry_test_hh_compile.log
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
