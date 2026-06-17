#!/usr/bin/env bash
set -euo pipefail

# A *fused* method call `proxy.method(arg)` on a Proxy receiver must route the
# call through the proxy (Get(proxy, method) + Call(method, proxy, args)),
# not misclassify the proxy's small registered id as a native-module integer
# handle. Proxy ids encode to small pointer-tagged values in the runtime's
# `< 0x100000` "small handle" band; the generic dynamic dispatcher
# (`js_native_call_method`) used to fall straight into that handle path for a
# fused call, dropping the argument and returning null/undefined.
#
# The decomposed form `const f = proxy.method; f(arg)` always worked (the read
# goes through `js_proxy_get`); only the fused form was broken. drizzle's
# relational-query mapper does `decoder.mapFromDriverValue(value)` where
# `decoder` is an aliased-column proxy, so every `findMany` column came back
# null. Regression for fix/proxy-fused-method-call (follow-up to #4661).

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
// The receiver must be opaque (`any`) and the method reached via the dynamic
// dispatcher for the bug to fire. An own *function-valued property* on the
// target (rather than a statically-resolvable class method) keeps the static
// analyzer from binding the call to a known vtable, so codegen emits the
// generic `js_native_call_method` dispatch — exactly the drizzle shape.
function makeForwardingProxy(): any {
  const target: any = { kind: "col" };
  target.run = function (v: any) { return v; };
  return new Proxy(target, { get(t: any, k: any) { return t[k]; } });
}

// 1) Forwarding get-trap proxy: fused call must not drop the argument.
const p: any = makeForwardingProxy();
const fused = p.run("hi");
if (fused !== "hi") throw new Error("fused get-trap proxy call dropped arg: " + JSON.stringify(fused));

// The decomposed form must keep working too.
const f = p.run; const decomp = f("hi");
if (decomp !== "hi") throw new Error("decomposed proxy call wrong: " + JSON.stringify(decomp));

// 2) No get trap -> forward to target; fused call still returns the arg.
function makePassthroughProxy(): any {
  const target: any = { kind: "col2" };
  target.echo = function (v: any) { return v; };
  return new Proxy(target, {});
}
const p2: any = makePassthroughProxy();
if (p2.echo("yo") !== "yo") throw new Error("fused no-trap proxy call dropped arg: " + JSON.stringify(p2.echo("yo")));

// 3) `this` is bound to the proxy inside the fused call.
function makeThisProxy(): any {
  const target: any = { tag: "T" };
  target.readTag = function (this: any) { return this.tag; };
  return new Proxy(target, { get(t: any, k: any) { return t[k]; } });
}
const p3: any = makeThisProxy();
if (p3.readTag() !== "T") throw new Error("`this` not bound to proxy in fused call: " + JSON.stringify(p3.readTag()));

// 4) Inherited method through the target's prototype chain (the real drizzle
//    case: `mapFromDriverValue` is inherited from a base class).
class Base { mapFromDriverValue(v: any) { return v; } }
class Mid extends Base { x = 1; }
class Leaf extends Mid { y = 2; }
function makeInheritedProxy(): any {
  const inst: any = new Leaf();
  return new Proxy(inst, { get(t: any, k: any) { return t[k]; } });
}
const p4: any = makeInheritedProxy();
if (p4.mapFromDriverValue("hi") !== "hi") {
  throw new Error("fused call to inherited method dropped arg: " + JSON.stringify(p4.mapFromDriverValue("hi")));
}

// 5) Multiple args are all forwarded.
function makeSumProxy(): any {
  const target: any = {};
  target.add3 = function (a: number, b: number, c: number) { return a + b + c; };
  return new Proxy(target, { get(t: any, k: any) { return t[k]; } });
}
const p5: any = makeSumProxy();
if (p5.add3(1, 2, 3) !== 6) throw new Error("fused multi-arg call wrong: " + JSON.stringify(p5.add3(1, 2, 3)));

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: fused method call on a Proxy receiver forwards arg + this (not dropped)"
