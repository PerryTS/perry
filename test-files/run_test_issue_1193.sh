#!/bin/bash
# Wire-level regression for issue #1193 — cheerio chain through any-typed
# intermediates. See test_issue_1193_cheerio_chain.ts for the writeup.
#
# Usage:
#   PERRY_BIN=./target/release/perry ./test-files/run_test_issue_1193.sh
#
# Assertions:
#   * Bound-then-method chain through CheerioSelectionHandle works:
#     `.text()` returns "helloworld".
#   * Bound document, then `.select(...).text()`: same result via the
#     document-level arm of dispatch_cheerio.
#
# Pre-fix the runtime threw `(number).<method> is not a function` on the
# `.text()` step the moment the SelectionHandle landed in a `let` binding.

set -euo pipefail

PERRY_BIN="${PERRY_BIN:-./target/release/perry}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_SRC="$SCRIPT_DIR/test_issue_1193_cheerio_chain.ts"
EXE="${TMPDIR:-/tmp}/test_issue_1193_cheerio_chain"

cd "$WORKSPACE_ROOT"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL: perry binary not found at $PERRY_BIN — build first via cargo build --release -p perry" >&2
    exit 1
fi

echo "[1193] compiling fixture..."
PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY_BIN" "$TEST_SRC" -o "$EXE" >/dev/null

echo "[1193] running fixture..."
OUTPUT="$("$EXE" 2>&1)"
echo "$OUTPUT" | sed 's/^/    /'

fail=0
if [[ "$OUTPUT" != *"text: helloworld"* ]]; then
    echo "[1193] FAIL -- selection text() regressed (expected 'helloworld')"
    fail=1
fi
if [[ "$OUTPUT" != *"text2: helloworld"* ]]; then
    echo "[1193] FAIL -- doc.select(...).text() through bound doc regressed"
    fail=1
fi
if [[ "$OUTPUT" == *"is not a function"* ]]; then
    echo "[1193] FAIL -- dispatch fell through to the not-callable path"
    fail=1
fi

if [[ $fail -eq 0 ]]; then
    echo "[1193] PASS"
    exit 0
else
    exit 1
fi
