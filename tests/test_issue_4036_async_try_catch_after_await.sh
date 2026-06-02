#!/bin/bash
# Regression for #4036: a try/catch that appears after an earlier await in an
# async function must still receive synchronous throws from the post-await
# continuation.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY:-$REPO_ROOT/target/release/perry}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

COMPILE_ENV=(env PERRY_ALLOW_UNIMPLEMENTED=1)
if [[ -f "$REPO_ROOT/target/debug/libperry_runtime.a" || -f "$REPO_ROOT/target/release/libperry_runtime.a" ]]; then
    COMPILE_ENV=(env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1)
fi

cat > "$TMPDIR/main.ts" << 'EOF'
function boom() {
  throw "call-boom";
}

function syncCatch() {
  try {
    throw "sync-boom";
  } catch (e: any) {
    console.log("sync caught:", e);
  }
}

async function noPriorAwait() {
  try {
    throw "no-await-boom";
  } catch (e: any) {
    console.log("no-await caught:", e);
  }
}

async function directPostAwait() {
  await 0;
  try {
    throw "direct-boom";
  } catch (e: any) {
    console.log("direct caught:", e);
  }
}

async function callPostAwait() {
  await 0;
  try {
    boom();
  } catch (e: any) {
    console.log("call caught:", e);
  }
}

async function promiseResolvePostAwait() {
  await Promise.resolve(1);
  try {
    throw new Error("promise-boom");
  } catch (e: any) {
    console.log("promise caught:", e.message);
  }
}

async function rejectedAwaitStillWorks() {
  try {
    await Promise.reject("reject-boom");
    console.log("reject unexpectedly resolved");
  } catch (e: any) {
    console.log("reject caught:", e);
  }
}

syncCatch();
await noPriorAwait();
await directPostAwait();
await callPostAwait();
await promiseResolvePostAwait();
await rejectedAwaitStillWorks();
console.log("done");
EOF

"${COMPILE_ENV[@]}" "$PERRY" compile --no-cache "$TMPDIR/main.ts" -o "$TMPDIR/test_bin" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

RUN_OUTPUT="$("$TMPDIR/test_bin" 2>&1)"

EXPECTED="sync caught: sync-boom
no-await caught: no-await-boom
direct caught: direct-boom
call caught: call-boom
promise caught: promise-boom
reject caught: reject-boom
done"

if [[ "$RUN_OUTPUT" == "$EXPECTED" ]]; then
    echo "PASS"
    exit 0
fi

echo "FAIL: async post-await try/catch output changed"
echo "Expected:"
echo "$EXPECTED"
echo
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
