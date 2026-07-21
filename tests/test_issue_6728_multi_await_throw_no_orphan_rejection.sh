#!/bin/bash
# Regression coverage for #6728: an async function that reaches a `throw` after
# TWO-OR-MORE `await`s must reject exactly ONE promise (its result, caught by a
# surrounding try/catch). Before the fix, each await past the first minted an
# intermediate then-result promise that adopted the throw's rejection with no
# handler attached, surfacing a spurious "Uncaught (in promise)" and exiting the
# process non-zero — even though the throw was already handled. This is the pi
# interactive-TUI blocker (the Anthropic SDK's makeRequest throws its error
# after many awaits).

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

COMPILE_ENV=()
if [ -f "$SCRIPT_DIR/../target/debug/libperry_runtime.a" ] || [ -f "$SCRIPT_DIR/../target/release/libperry_runtime.a" ]; then
  COMPILE_ENV=(env PERRY_NO_AUTO_OPTIMIZE=1)
fi

cat > "$TMPDIR/main.ts" << 'EOF'
function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

// >=2 awaits before the throw is the trigger; 0 or 1 await never orphaned.
async function twoAwaitsThenThrow(): Promise<void> {
  await sleep(1);
  await sleep(1);
  throw new Error("boom");
}

// Nested async layer (mirrors the SDK: makeRequest awaited by a caller).
async function callLayer(): Promise<void> {
  await twoAwaitsThenThrow();
}

async function main(): Promise<void> {
  try {
    await callLayer();
    console.log("no throw (wrong)");
  } catch (e: any) {
    console.log("caught " + (e && e.message));
  }
  console.log("done");
}

main();
EOF

cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)
RUN_EXIT=$?

EXPECTED="caught boom
done"

if [ "$RUN_OUTPUT" = "$EXPECTED" ] && [ "$RUN_EXIT" -eq 0 ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: multi-await throw produced an orphaned rejection (#6728 regression)"
echo "Expected (exit 0):"
echo "$EXPECTED"
echo ""
echo "Got (exit $RUN_EXIT):"
echo "$RUN_OUTPUT"
exit 1
