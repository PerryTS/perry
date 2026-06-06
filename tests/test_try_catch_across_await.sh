#!/bin/bash
# Regression: a `try/catch` (or `try/finally`) that SPANS an `await` leaked one
# runtime exception-handler slot (`try_depth`) every time it executed. The async
# state machine lowers `await` to a suspend that `break`s out of the dispatch
# loop's real `try { ... }`; that break skipped the matching `js_try_end`, so
# `try_depth` climbed by one per awaited try/catch and the runtime aborted with
# "Try block nesting too deep" at MAX_TRY_DEPTH = 128. This killed any
# long-running async server whose handlers `try { await ... } catch`.
#
# Fix: codegen now emits a compensating `js_try_end` for every open `try` frame
# a `break`/`continue`/labeled-break/labeled-continue jumps OUT of (the same
# balancing `Stmt::Return` already did), so `try_depth` nets to zero across the
# suspend boundary while keeping the catch/finally reachable.
#
# Coverage:
#   (a) long NON-throwing awaited try/catch loop (the leak path — no throw needed)
#   (b) throw-after-await still caught by the enclosing try
#   (c) finally still runs across an await
#   (d) nested awaited try/catch

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY:-${PERRY_BIN:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat > "$TMPDIR/main.ts" << 'EOF'
// (a) Long non-throwing awaited try/catch loop. Pre-fix this leaks one
//     try_depth slot per iteration and aborts before iteration 128.
async function noThrow(i: number): Promise<number> {
  try { await Promise.resolve(i); return i; } catch { return -1; }
}

// (b) A throw AFTER an await inside the try must still reach this try's catch.
async function withThrow(i: number): Promise<number> {
  try { await Promise.reject(new Error("boom")); return i; } catch { return -1; }
}

// (c) finally must run across an await (and on every iteration, no leak).
let finallyRuns = 0;
async function withFinally(i: number): Promise<number> {
  try { await Promise.resolve(i); return i; } finally { finallyRuns++; }
}

// (d) Nested awaited try/catch: inner throw-after-await caught by inner catch,
//     outer try/catch also spans an await. Run many times to exercise the leak.
async function nested(i: number): Promise<number> {
  try {
    await Promise.resolve(i);
    try {
      await Promise.reject(new Error("inner"));
      return 999;
    } catch {
      return i;
    }
  } catch {
    return -2;
  }
}

async function main(): Promise<void> {
  let sumNoThrow = 0;
  for (let i = 0; i < 400; i++) { sumNoThrow += await noThrow(i); }
  console.log("noThrow sum=" + sumNoThrow);

  let sumThrow = 0;
  for (let i = 0; i < 400; i++) { sumThrow += await withThrow(i); }
  console.log("withThrow sum=" + sumThrow);

  let sumFinally = 0;
  for (let i = 0; i < 400; i++) { sumFinally += await withFinally(i); }
  console.log("withFinally sum=" + sumFinally + " finallyRuns=" + finallyRuns);

  let sumNested = 0;
  for (let i = 0; i < 400; i++) { sumNested += await nested(i); }
  console.log("nested sum=" + sumNested);

  console.log("DONE");
}
main().catch((e) => console.log("ERR: " + String((e as any)?.message ?? e)));
EOF

"$PERRY" compile "$TMPDIR/main.ts" --output "$TMPDIR/test_bin" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -60
    exit 1
}

RUN_OUTPUT="$("$TMPDIR/test_bin" 2>&1)"

# noThrow returns i for each i in 0..399 → sum 0..399 = 79800
# withThrow returns -1 for each of 400 → -400
# withFinally returns i for each i in 0..399 → 79800, finally runs 400 times
# nested: inner throw-after-await is caught by inner catch which returns i → sum 79800
EXPECTED="noThrow sum=79800
withThrow sum=-400
withFinally sum=79800 finallyRuns=400
nested sum=79800
DONE"

if [[ "$RUN_OUTPUT" == "$EXPECTED" ]]; then
    echo "PASS"
    exit 0
fi

echo "FAIL: try/catch-across-await output changed (leak or wrong semantics)"
echo "Expected:"
echo "$EXPECTED"
echo
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
