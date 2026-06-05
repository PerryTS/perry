#!/usr/bin/env bash
set -euo pipefail

# Reading a static property off a cross-module *imported function* must read the
# property — not invoke the function. PropertyGet on an `Expr::ExternFuncRef`
# unconditionally called the `perry_fn_<src>__<name>()` getter (correct only for
# an exported const/let, whose symbol IS a value getter). For an imported
# *function*, that symbol is the function body, so every `Fn.staticProp` read
# invoked the function with zero args and read the property off its return value
# (undefined). Stripe hit this on `StripeResource.method` / `.extend` — an
# `export { StripeResource }` function carrying static props — so each static
# read constructed a StripeResource instead.

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

cat >"$TMPDIR/res.ts" <<'TS'
function StripeResource(this: any, stripe: any, deprecated: any) {
  if (deprecated) throw new Error("invoked with a 2nd arg: " + typeof deprecated);
  this._stripe = stripe;
}
(StripeResource as any).method = function () { return "M"; };
(StripeResource as any).extend = function () { return "E"; };
(StripeResource as any).MAX = 42;
(StripeResource as any).prototype = { _stripe: null, path: "", initialize() {} };
export { StripeResource };
TS
cat >"$TMPDIR/main.ts" <<'TS'
import { StripeResource } from "./res.js";
// Reads must NOT invoke the constructor.
if (typeof (StripeResource as any).method !== "function") throw new Error("method: " + typeof (StripeResource as any).method);
if (typeof (StripeResource as any).extend !== "function") throw new Error("extend: " + typeof (StripeResource as any).extend);
if ((StripeResource as any).MAX !== 42) throw new Error("MAX: " + (StripeResource as any).MAX);
// Built-in function props read off the imported function too.
if ((StripeResource as any).name !== "StripeResource") throw new Error("name: " + (StripeResource as any).name);
// Reading the static into a local then calling it (the Stripe pattern:
// `const stripeMethod = StripeResource.method;`) dispatches correctly.
const m: any = (StripeResource as any).method;
if (m() !== "M") throw new Error("method via local: " + m());
const e: any = (StripeResource as any).extend;
if (e() !== "E") throw new Error("extend via local: " + e());
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/main.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: imported function static property read (no spurious invocation)"
