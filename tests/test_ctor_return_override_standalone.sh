#!/usr/bin/env bash
set -euo pipefail

# ECMAScript constructor return-override: a constructor that explicitly returns
# an object (or function) makes `new C()` evaluate to that object, not the
# freshly-allocated `this`. The default codegen path calls a shared standalone
# `<class>_constructor` symbol (opt out with PERRY_INLINE_CTOR=1); that path
# discarded the ctor's return value and always yielded the empty default
# instance. chalk's `class Chalk { constructor(o){ return chalkFactory(o); } }`
# (no-constructor-return) depends on the function being returned.

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
// Constructor returns a plain object.
class W1 { constructor() { return { kind: "obj" }; } }
const a: any = new W1();
if (typeof a !== "object" || a.kind !== "obj") throw new Error("W1: " + typeof a + " " + a.kind);

// Constructor returns a function (chalk's no-constructor-return shape).
function makeFn() { const f: any = (x: string) => "F:" + x; f.tag = "T"; return f; }
class W2 { constructor() { return makeFn(); } }
const b: any = new W2();
if (typeof b !== "function" || b.tag !== "T" || b("x") !== "F:x") throw new Error("W2: " + typeof b);

// Ordinary constructor (implicit return this) is unaffected.
class P { x: number; constructor(v: number) { this.x = v; } }
const p = new P(5);
if (p.x !== 5) throw new Error("P: " + p.x);

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: constructor return-override honored on the standalone-symbol path"
