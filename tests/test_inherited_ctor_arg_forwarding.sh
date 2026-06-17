#!/usr/bin/env bash
set -euo pipefail

# `new Sub(arg)` where Sub has NO own constructor but its parent does must forward
# `arg` to the parent ctor (JS implicit `constructor(...args){ super(...args) }`).
# The recursion-guarded "symbol-call" construction path (taken when `new Sub(...)`
# appears inside a method of Sub) computed the ctor arity from Sub's own ctor —
# `None` → 0 — so it dropped every argument; the synthesized ctor's forwarded
# params then read uninitialized and the inherited `this.x = arg` stored garbage.
# Pervasive in zod: `new ZodNumber({checks:[...]})` from `_addCheck` (ZodNumber has
# no own ctor, ZodType does) made `_def` undefined → `[...this._def.checks]` threw.

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

cat >"$TMPDIR/c.ts" <<'TS'
abstract class Base {
  _def: any;
  constructor(def: any) { this._def = def; }
  abstract _p(): any;
}
class Num extends Base {              // no own constructor — inherits Base's
  _p() { return 1; }
  // `new Num(...)` issued INSIDE a Num method -> recursion-guarded symbol-call path
  addCheck(c: any): Num { return new Num({ ...this._def, checks: [...this._def.checks, c] }); }
  static create = (): Num => new Num({ checks: [], tn: "N" });
}

const a: any = Num.create();
if (JSON.stringify(a._def) !== '{"checks":[],"tn":"N"}') throw new Error("create _def: " + JSON.stringify(a._def));
const b: any = a.addCheck({ kind: "int" });
if (JSON.stringify(b._def.checks) !== '[{"kind":"int"}]') throw new Error("addCheck _def: " + JSON.stringify(b._def));
const c: any = a.addCheck({ kind: "int" }).addCheck({ kind: "min" });
if (c._def.checks.length !== 2) throw new Error("chain: " + JSON.stringify(c._def.checks));
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/c.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: inherited-ctor argument forwarding"
