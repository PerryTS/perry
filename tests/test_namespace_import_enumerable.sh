#!/usr/bin/env bash
set -euo pipefail

# A namespace import (`import * as ns from "./m.js"`) used as a whole VALUE must
# be a real object whose OWN ENUMERABLE properties are the source module's
# RUNTIME exports — so `Object.keys(ns)`, `for…in ns`, `Object.entries(ns)`,
# `hasOwnProperty`, and passing `ns` to a library all see the members. Pre-fix
# the namespace value lowered to an empty `js_unresolved_namespace_stub`, so
# `Object.keys` was `[]` (drizzle's `drizzle(pool, { schema })` where
# `import * as schema` saw zero tables; Stripe's `_prepResources`
# `for (const n in resources)` attached nothing).
#
# Type-only exports (`export type` / `export interface`) are erased at runtime
# and must NOT appear among the enumerable members (they have no runtime value).

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/schema.ts" <<'TS'
export const alpha = { id: 1 };
export const beta = { id: 2 };
export const gammaEnum = ["x", "y"] as const;
export function helper() { return "H"; }
// Type-only exports — erased at runtime, must not be enumerable members.
export type Alpha = typeof alpha;
export type Beta = typeof beta;
export interface Thing { id: number }
TS
cat >"$TMPDIR/main.ts" <<'TS'
import * as schema from "./schema.js";

const keys = Object.keys(schema).sort();
if (JSON.stringify(keys) !== '["alpha","beta","gammaEnum","helper"]') {
  throw new Error("keys: " + JSON.stringify(keys));
}
// for…in matches.
const forIn: string[] = [];
for (const k in schema) forIn.push(k);
if (JSON.stringify(forIn.sort()) !== '["alpha","beta","gammaEnum","helper"]') {
  throw new Error("for-in: " + JSON.stringify(forIn));
}
// hasOwnProperty + Object.entries.
if (!Object.prototype.hasOwnProperty.call(schema, "alpha")) throw new Error("hasOwn alpha");
if (Object.prototype.hasOwnProperty.call(schema, "Alpha")) throw new Error("type leaked: Alpha");
const entries = Object.entries(schema).map(([k]) => k).sort();
if (entries.length !== 4) throw new Error("entries: " + JSON.stringify(entries));
// Members still resolve to their real values + the direct `ns.member` read works.
if ((schema as any).alpha.id !== 1) throw new Error("alpha.id: " + (schema as any).alpha.id);
if ((schema as any).helper() !== "H") throw new Error("helper(): " + (schema as any).helper());
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/main.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: namespace import is an enumerable runtime-export object"
