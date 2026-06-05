#!/usr/bin/env bash
set -euo pipefail

# `Function.prototype.bind` invoked as a *value* (not a `fn.bind(...)` method
# call) must create a bound function. It was installed as a no-op proto method,
# so `Reflect.apply(Function.prototype.bind, fn, [thisArg])` and
# `Function.prototype.bind.apply(fn, …)` returned `undefined`. The
# `Function.prototype.call.bind(method)` uncurry-this idiom in
# `call-bind-apply-helpers` (call-bound → side-channel → qs → Stripe) relies on
# `Reflect.apply(bind, call, [fn])` returning a real bound function.

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

cat >"$TMPDIR/f.ts" <<'TS'
const bind = Function.prototype.bind;
const call = Function.prototype.call;

// uncurry-this: `Function.prototype.call.bind(String.prototype.indexOf)`
// built via Reflect.apply(bind, call, [indexOf]).
const indexOf = String.prototype.indexOf;
const boundCall: any = (Reflect as any).apply(bind, call, [indexOf]);
if (typeof boundCall !== "function") throw new Error("not a function: " + typeof boundCall);
// boundCall("hello", "l") === "hello".indexOf("l") === 2
if (boundCall("hello", "l") !== 2) throw new Error("uncurry: " + boundCall("hello", "l"));

// Also via Function.prototype.bind.apply
function add(a: number, b: number) { return a + b; }
const g: any = (bind as any).apply(add, [null, 10]);   // add.bind(null, 10)
if (typeof g !== "function") throw new Error("bind.apply not fn: " + typeof g);
if (g(5) !== 15) throw new Error("bind.apply partial: " + g(5));

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: Function.prototype.bind as a value (Reflect.apply / .apply)"
