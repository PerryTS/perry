#!/bin/bash
# Regression: a double-optional chain that calls a method on the result of an
# upstream optional member access — `a?.b?.method(args)` — threw
# `TypeError: Cannot read properties of undefined (reading 'method')` when the
# upstream `a.b` was `undefined`, instead of short-circuiting to `undefined`.
#
# Root cause: in the HIR optional-call lowering (lower_expr.rs OptChain(Call)),
# when the receiver of an optional method member (`?.method`) is itself produced
# by an upstream optional chain, the receiver lowers to a Conditional. The
# inner-Conditional nesting branch reused the un-short-circuited receiver
# (`a.b`) directly as the call's object WITHOUT re-applying the per-receiver
# null-guard that the non-Conditional path emits — so `(a.b).method(args)`
# dereferenced an `undefined` receiver and threw. The fix re-adds a
# receiver-nullish guard (`a.b == null ? undefined : (a.b).method(args)`) for
# the optional-member case, gated on a side-effect-free receiver.
#
# This is the `_?.allowModels?.some(...)` startup wall.

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

cat > "$TMPDIR/main.ts" << 'EOF'
const cfg: any = {};                 // cfg.allowModels is undefined
const u: any = undefined;
const obj: any = { a: { arr: [1, 2, 3] } };

// The wall: optional member access feeding an optional-member method call.
// cfg.allowModels is undefined, so `?.some(...)` must short-circuit, not throw.
const r1 = cfg?.allowModels?.some((m: string) => m === "x");
console.log("r1=" + (r1 === undefined ? "undefined" : r1));  // undefined

// Plain undefined receiver via a double chain.
const r2 = u?.missing?.some((x: number) => x > 0);
console.log("r2=" + (r2 === undefined ? "undefined" : r2));  // undefined

// Positive path: the chain resolves to a real array → method runs.
const r3 = obj?.a?.arr?.some((x: number) => x === 2);
console.log("r3=" + r3);                                      // true

// Positive path with a transform method (no false short-circuit).
const r4 = obj?.a?.arr?.map((x: number) => x * 2).join(",");
console.log("r4=" + r4);                                      // "2,4,6"

// Optional-CALL variant: `a?.b?.method?.(args)` where `a.b` is undefined —
// the function-value guard must NOT read `(a.b).method` off the undefined
// receiver (which would throw); the receiver short-circuit comes first.
const r5 = u?.missing?.foo?.((x: number) => x > 0);
console.log("r5=" + (r5 === undefined ? "undefined" : r5));  // undefined

// Optional-CALL variant, positive: real function value resolves and is called.
const o2: any = { fn: () => 99 };
const r6 = o2?.fn?.();
console.log("r6=" + r6);                                      // 99
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts --output test_bin >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED='r1=undefined
r2=undefined
r3=true
r4=2,4,6
r5=undefined
r6=99'

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: double-optional member call short-circuit returned wrong value"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
