#!/bin/bash
# Regression for #684: type-only namespace imports must not register their
# source module's exports into `import_function_prefixes` (or
# `namespace_member_prefixes`).
#
# Bug: `import type * as Schema from "./Schema.js"` was iterated like a
# value namespace import in compile.rs's resolution loop, so Schema's
# exports were registered into the flat name→source-prefix map. When the
# same module also had a real `import { TaggedError } from "./Data.js"`
# AND Schema.ts also exported `TaggedError`, HashMap iteration order
# determined the winner — and when Schema's entry won, top-level
# `TaggedError(...)` dispatched into Schema's TaggedError instead of
# Data's. Worse, Schema is type-only so it isn't in `module_init_deps`
# either, meaning its backing global is still 0.0 — the call threw
# `TypeError: value is not a function` from `js_closure_call1`.
#
# Fix: skip whole-decl type-only imports in compile.rs's resolution loop
# (mirroring the L3234 `module_init_deps` filter from #680).

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

# Data.ts: the REAL `kindOf` that the importer wants.
cat > "$TMPDIR/Data.ts" << 'EOF'
export const kindOf = (x: unknown): string => "data:" + typeof x;
EOF

# Schema.ts: a DIFFERENT `kindOf` that exists only so the type-only
# import can collide. Importantly, Schema.ts is NOT initialized at
# runtime (no value-position reference), so `Schema_ts__init` never
# runs and the backing global stays uninitialized. Pre-fix, the
# importer would call _into_ this uninitialized symbol and throw
# `TypeError: value is not a function`.
cat > "$TMPDIR/Schema.ts" << 'EOF'
export const kindOf = (_x: unknown): string => {
  throw new Error("Schema.kindOf must never be reached at runtime");
};
EOF

# main.ts: imports `kindOf` as a value FROM Data, AND imports Schema
# as a TYPE-ONLY namespace. The type-only line should be erased at
# runtime — `kindOf(42)` must dispatch to Data's kindOf and return
# the expected `"data:number"`. Pre-fix, Schema.ts's type-only import
# leaked into the name→prefix map and (depending on HashMap order)
# the call landed on Schema_ts's uninitialized global slot.
cat > "$TMPDIR/main.ts" << 'EOF'
import { kindOf } from "./Data";
import type * as Schema from "./Schema";

console.log(kindOf(42));
console.log(kindOf("hi"));

// Reference the type-only binding so TS keeps the import, but only
// in type position — the resulting JS is a noop.
type _Unused = ReturnType<typeof Schema.kindOf>;
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts --output test_bin >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED="data:number
data:string"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: #684 type-only namespace collision still leaks"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
