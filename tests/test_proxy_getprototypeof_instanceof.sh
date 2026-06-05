#!/usr/bin/env bash
set -euo pipefail

# A Proxy without a `getPrototypeOf` trap must forward `[[GetPrototypeOf]]` to its
# target, and `proxy instanceof C` must follow that forwarded chain. Perry
# represents a Proxy as a small registered id (not a heap object), so
# `Object.getPrototypeOf(proxy)` returned `null` and `proxy instanceof C` was
# always `false`. Drizzle aliases columns as `new Proxy(column, …)` and its
# brand check `is(value, type)` reads `getPrototypeOf(value).constructor` / does
# `value instanceof type` — the null prototype crashed on `null.constructor`.

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
class Base { static tag = "B"; }
class Mid extends Base {}
class Leaf extends Mid { constructor(public n: string) { super(); } }

const leaf = new Leaf("x");
const handler = { get(t: any, k: any) { return t[k]; } };
const proxy: any = new Proxy(leaf, handler);

// getPrototypeOf forwards to the target's prototype (constructor = the class).
const proto = Object.getPrototypeOf(proxy);
if (proto === null || proto === undefined) throw new Error("getPrototypeOf(proxy) was nullish");
if (proto.constructor?.name !== "Leaf") throw new Error("proto.constructor: " + proto.constructor?.name);

// instanceof follows the forwarded chain (target's class hierarchy).
if (!(proxy instanceof Leaf)) throw new Error("proxy not instanceof Leaf");
if (!(proxy instanceof Mid)) throw new Error("proxy not instanceof Mid");
if (!(proxy instanceof Base)) throw new Error("proxy not instanceof Base");
if (!(proxy instanceof Object)) throw new Error("proxy not instanceof Object");

// member access still forwards through the get trap.
if (proxy.n !== "x") throw new Error("proxy.n: " + proxy.n);

// nested proxy (proxy of a proxy) still resolves to the real target.
const proxy2: any = new Proxy(proxy, handler);
if (!(proxy2 instanceof Leaf)) throw new Error("nested proxy not instanceof Leaf");
if (Object.getPrototypeOf(proxy2)?.constructor?.name !== "Leaf") throw new Error("nested getPrototypeOf");

// the `is()`-style brand check no longer crashes on null.constructor.
function is(value: any, type: any): boolean {
  if (!value || typeof value !== "object") return false;
  if (value instanceof type) return true;
  let cls = Object.getPrototypeOf(value)?.constructor;
  while (cls) cls = Object.getPrototypeOf(cls);
  return false;
}
if (!is(proxy, Base)) throw new Error("is(proxy, Base) was false");

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: proxy getPrototypeOf forwards to target + instanceof follows it"
