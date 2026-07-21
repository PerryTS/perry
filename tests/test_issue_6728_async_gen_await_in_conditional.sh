#!/bin/bash
# Regression coverage for #6728 / #6709 completion: an `await` nested inside an
# `if` (or any control-flow construct whose branch has no `yield`) in an
# async generator must SUSPEND on the microtask queue, not busy-wait. This is
# pi's EventStream push/pull async iterator shape:
#     while (true) {
#       if (queue.length === 0) { if (done) return; await producerPromise; }
#       while (queue.length > 0) yield queue.shift();
#     }
# Before the fix the `await` inside the `if` was never split into a suspend
# state (the split gate only checked for `yield`), so it deadlocked whenever the
# awaited Promise was settled by an external producer (here: a timer).

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
class EventStream {
  queue: string[] = [];
  resolve: (() => void) | null = null;
  done = false;
  push(e: string) { this.queue.push(e); if (this.resolve) { const r = this.resolve; this.resolve = null; r(); } }
  async *[Symbol.asyncIterator](): AsyncGenerator<string> {
    while (true) {
      if (this.queue.length === 0) {              // await nested in `if` — no yield here
        if (this.done) return;
        await new Promise<void>((r) => { this.resolve = r; }); // settled by push() from a timer
      }
      while (this.queue.length > 0) yield this.queue.shift() as string;
    }
  }
}
const stream = new EventStream();
(async () => {
  for await (const e of stream) {
    console.log("event " + e);
    if (e === "last") { console.log("got last"); process.exit(0); }
  }
})();
setTimeout(() => stream.push("working"), 50);
setTimeout(() => stream.push("token"), 100);
setTimeout(() => stream.push("last"), 150);
setTimeout(() => { console.log("GUARD (deadlocked)"); process.exit(2); }, 3000);
EOF

cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)
RUN_EXIT=$?

EXPECTED="event working
event token
event last
got last"

if [ "$RUN_OUTPUT" = "$EXPECTED" ] && [ "$RUN_EXIT" -eq 0 ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: async-generator await-in-conditional deadlocked (#6728/#6709 regression)"
echo "Expected (exit 0):"
echo "$EXPECTED"
echo ""
echo "Got (exit $RUN_EXIT):"
echo "$RUN_OUTPUT"
exit 1
