#!/usr/bin/env bash
set -euo pipefail

# A TS `namespace`'s `export const` members must be accessible when the namespace
# is imported into ANOTHER module. Namespace FUNCTIONS lowered as static methods
# already crossed the module boundary, but `export const` members lived only in
# the defining module's per-module `namespace_vars`, so an importer read them back
# as undefined/garbage. zod's `util` namespace (`util.objectKeys`, …) is imported
# this way, so `_getCached` got null keys and every `z.object({...}).parse()`
# silently dropped all fields.

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

cat >"$TMPDIR/util_mod.ts" <<'TS'
export namespace util {
  export function fn() { return "FN"; }
  export const objectKeys = (obj: any) => Object.keys(obj);
  export const arrow = () => "ARROW";
  export const num = 42;
}
TS
cat >"$TMPDIR/main.ts" <<'TS'
import { util } from "./util_mod.js";
if (util.fn() !== "FN") throw new Error("fn: " + util.fn());
if (util.arrow() !== "ARROW") throw new Error("arrow: " + util.arrow());
if (util.num !== 42) throw new Error("num: " + util.num);
if (JSON.stringify(util.objectKeys({ a: 1, b: 2 })) !== '["a","b"]') {
  throw new Error("objectKeys: " + JSON.stringify(util.objectKeys({ a: 1, b: 2 })));
}
// assigning the member to a local and calling it must also work
const ok = util.objectKeys;
if (JSON.stringify(ok({ x: 1 })) !== '["x"]') throw new Error("via local: " + JSON.stringify(ok({ x: 1 })));
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/main.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: namespace const cross-module access"
