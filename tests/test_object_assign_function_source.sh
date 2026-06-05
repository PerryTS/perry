#!/usr/bin/env bash
set -euo pipefail

# `Object.assign(target, fn)` where the source is a function/closure must copy
# the function's own *enumerable* statics (and skip the non-enumerable
# `length`/`name`/`prototype` slots) without hanging. A function is a
# ClosureHeader, not an ObjectHeader; reading `keys_array` off it yielded a
# garbage key_count and a runaway copy loop. Stripe's `protoExtend` does
# `Object.assign(Constructor, Super)` to copy a resource class's enumerable
# statics — this hung at `import 'stripe'`.

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
function Super(this: any, a: any) { this._a = a; }
(Super as any).extend = "EXT";
(Super as any).method = "METH";
(Super as any).MAX = 100;
// Reassigning `.prototype` to a plain object is what Stripe's protoExtend does.
(Super as any).prototype = { _a: null, path: "", initialize() {}, foo() { return 1; } };

const t: any = {};
Object.assign(t, Super);
// Enumerable statics copied:
if (t.extend !== "EXT") throw new Error("extend: " + t.extend);
if (t.method !== "METH") throw new Error("method: " + t.method);
if (t.MAX !== 100) throw new Error("MAX: " + t.MAX);
// Non-enumerable `prototype`/`name`/`length` must NOT be copied.
const keys = Object.keys(t).sort();
if (JSON.stringify(keys) !== '["MAX","extend","method"]') throw new Error("keys: " + JSON.stringify(keys));
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: Object.assign with a function source copies enumerable statics"
