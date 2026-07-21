#!/bin/bash
# Regression coverage for #6728: a CLASS-FIELD-INITIALIZER async arrow whose body
# contains `await` must run its body when called. Before the fix its async-step
# state locals were not boxed (the module-wide boxed-var scan skipped class field
# initializers), so calling it ran no body and the `await` on it resolved with no
# effect. This is pi's `_handleAgentEvent` shape — agent events never rendered.

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
class Session {
  steering: string[] = [];
  // class-field async arrow that uses `this`, an await, and a nested `if`
  handle = async (event: any) => {
    console.log("handle-enter " + event.type);
    if (event.type === "msg" && this.steering.length > 0) {
      this.steering.pop();
    }
    await Promise.resolve(1);
    console.log("handle-after-await " + event.type);
  };
}
// call it directly, via a reference, and via a Set (mirrors the listener path)
(async () => {
  const s = new Session();
  await s.handle({ type: "a" });
  const ref = s.handle;
  await ref({ type: "b" });
  const listeners = new Set<any>([s.handle]);
  for (const l of listeners) await l({ type: "msg" });
  console.log("done");
  process.exit(0);
})();
EOF

cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)
RUN_EXIT=$?

EXPECTED="handle-enter a
handle-after-await a
handle-enter b
handle-after-await b
handle-enter msg
handle-after-await msg
done"

if [ "$RUN_OUTPUT" = "$EXPECTED" ] && [ "$RUN_EXIT" -eq 0 ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: class-field async-arrow body did not run (#6728 regression)"
echo "Expected (exit 0):"
echo "$EXPECTED"
echo ""
echo "Got (exit $RUN_EXIT):"
echo "$RUN_OUTPUT"
exit 1
