#!/usr/bin/env bash
set -euo pipefail

# A method read as a value (`const f = obj.m`) must snapshot the method at READ
# time and keep invoking it even after `obj.m` is later reassigned. The canonical
# trigger is the ubiquitous `this.m = this.m.bind(this)` (zod's `ZodType` ctor,
# React class components, …): the own property `m` becomes the bound function
# whose target is the captured value, so re-resolving `m` by name at call time
# found the own property and recursed until the call-depth guard returned the
# null object — surfacing as `obj.m()` yielding `[object Object]` / `undefined`.

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

cat >"$TMPDIR/m.ts" <<'TS'
// 1. self-shadowing bind on an OWN method — the zod `ZodType` ctor pattern.
class A { tag = "a"; m() { return "M:" + this.tag; } constructor() { (this as any).m = (this as any).m.bind(this); } }
if (new A().m() !== "M:a") throw new Error("self-bind own: " + new A().m());

// 2. self-shadowing bind on an INHERITED method (ZodString extends ZodType).
class Base { m() { return "B:" + (this as any).tag; } constructor() { (this as any).m = (this as any).m.bind(this); } }
class Child extends Base { tag = "c"; }
if (new Child().m() !== "B:c") throw new Error("self-bind inherited: " + new Child().m());

// 3. capturing `this.m` then reassigning the own property keeps the method.
class C { tag = "t"; r(): string { const captured = (this as any).m; (this as any).m = "SHADOW"; return captured(); } m() { return "C:" + this.tag; } }
if (new C().r() !== "C:t") throw new Error("this-snapshot regressed: " + new C().r());

// 4. a real own-property ARROW override is STILL honored (no regression).
class D { tag = "d"; constructor() { (this as any).m = () => "OWN:" + this.tag; } m() { return "PROTO"; } }
if (new D().m() !== "OWN:d") throw new Error("arrow override regressed: " + new D().m());

// 5. plain Function.prototype.bind still works.
function plain(this: any, x: number) { return this.base + x; }
if (plain.bind({ base: 100 })(5) !== 105) throw new Error("plain bind");

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/m.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: method-value snapshot + self-bind"
