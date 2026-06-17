#!/usr/bin/env bash
set -euo pipefail

# Reading a SYMBOL-keyed property off a Proxy must forward through the proxy
# `get` trap (or to the target), not dereference the proxy's small registered id
# as a heap object — which was an EXC_BAD_ACCESS. drizzle's relational-query
# builder reads symbol keys (`col[entityKind]`, `col[Table.Symbol.*]`) off
# aliased-column proxies (`new Proxy(column, …)`), so every `findMany` crashed.

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
const tag = Symbol("tag");
const other = Symbol.for("shared");

// forwarding get trap
const target: any = { [tag]: "T", [other]: 42, plain: "p" };
const proxy: any = new Proxy(target, { get(t: any, k: any) { return t[k]; } });
if (proxy[tag] !== "T") throw new Error("symbol via get trap: " + String(proxy[tag]));
if (proxy[other] !== 42) throw new Error("Symbol.for via get trap: " + String(proxy[other]));
if (proxy.plain !== "p") throw new Error("string via get trap: " + proxy.plain);

// no get trap -> forward to target
const p2: any = new Proxy(target, {});
if (p2[tag] !== "T") throw new Error("symbol forward to target: " + String(p2[tag]));

// a class instance behind the proxy, read its static-ish symbol off the instance
class C { [tag] = "instTag"; }
const inst: any = new C();
const p3: any = new Proxy(inst, { get(t: any, k: any) { return t[k]; } });
if (p3[tag] !== "instTag") throw new Error("instance symbol via proxy: " + String(p3[tag]));

// missing symbol returns undefined, not a crash
if (proxy[Symbol("absent")] !== undefined) throw new Error("absent symbol should be undefined");

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: symbol-keyed read on a Proxy forwards (no EXC_BAD_ACCESS)"
