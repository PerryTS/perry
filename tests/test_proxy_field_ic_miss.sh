#!/usr/bin/env bash
set -euo pipefail

# Reading a STRING-keyed property off a Proxy through the monomorphic
# inline-cache *miss* handler (`js_object_get_field_ic_miss`) must route through
# the proxy `get` trap, not pass the proxy's small registered id to
# `closure_dynamic_prop_by_key` -> `is_closure_ptr`, which reads CLOSURE_MAGIC at
# offset 12 of the (fake) pointer and SIGSEGVs.
#
# drizzle-orm's relational query builder wraps each selected column in an
# aliased-column `new Proxy(column, handler)` and then reads string properties
# off it (`col.name`, `col.dataType`, ...) while assembling rows in
# `db.query.<table>.findMany()`. Those fused `proxy.field` reads miss the
# per-site inline cache (a Proxy has no stable `keys_array` shape), landing in
# the IC-miss handler with the proxy id as the receiver -> EXC_BAD_ACCESS at a
# proxy-band address like 0xf0024. Refs #4661/#4663 (same proxy-deref family).

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
// A Proxy wrapping a plain object with string-keyed fields, read through a
// forwarding get trap. The repeated reads in a loop drive the per-site inline
// cache to miss on the proxy receiver, exercising js_object_get_field_ic_miss.
const target: any = { name: "id", dataType: "string", notNull: true };
const proxy: any = new Proxy(target, {
  get(t: any, k: any) { return t[k]; },
});

let acc = "";
for (let i = 0; i < 1000; i++) {
  // Fused `proxy.field` string-keyed reads — these miss the inline cache.
  acc = String(proxy.name) + ":" + String(proxy.dataType) + ":" + String(proxy.notNull);
}
if (acc !== "id:string:true") throw new Error("string read via proxy: " + acc);

// No get trap -> forward to target directly through the same miss path.
const p2: any = new Proxy(target, {});
if (p2.name !== "id") throw new Error("string forward to target: " + String(p2.name));

// A missing string key returns undefined (not a crash).
if (proxy.absent !== undefined) throw new Error("absent string key should be undefined");

// Class instance behind the proxy, read an instance field by string key.
class Column { name = "col"; sql = "SELECT"; }
const inst: any = new Column();
const p3: any = new Proxy(inst, { get(t: any, k: any) { return t[k]; } });
let r = "";
for (let i = 0; i < 1000; i++) { r = String(p3.name) + "/" + String(p3.sql); }
if (r !== "col/SELECT") throw new Error("instance field via proxy: " + r);

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored (segfault?)"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: string-keyed read on a Proxy via IC-miss forwards (no EXC_BAD_ACCESS)"
