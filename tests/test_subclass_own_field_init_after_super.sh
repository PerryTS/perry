#!/usr/bin/env bash
set -euo pipefail

# A subclass with NO own constructor, extending a parent that HAS one, must still
# run its OWN field initializers (the implicit default ctor is
# `constructor(...args){ super(...args); <own field inits> }`). The inline
# construction path applied `UpToInclusive(inherited_ctor_class)` up front — which
# keeps `chain[0..=idx(inherited)]` and EXCLUDES the leaf — and never applied the
# leaf's fields after super(). So a subclass's own `field = <init>` never ran; the
# field read the raw-0 slot.
#
# zod hit this exactly: `class ZodObject extends ZodType { private _cached = null }`
# left `_cached` at 0, so `_getCached()`'s `this._cached !== null` was true
# (0 !== null) and returned 0; `_parse` destructured `{ keys }` off 0 and every
# `z.object({...}).parse()` silently dropped all fields.

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
abstract class Base {
  _def: any;
  constructor(def: any) { this._def = def; }
  abstract _p(): any;
}
class Leaf extends Base {                 // no own ctor; own field initializers
  _cached: any = null;
  _count = 7;
  _list: number[] = [1, 2];
  _p() { return 1; }
  static create = (): Leaf => new Leaf({ k: 1 });
}

const o: any = Leaf.create();
if (o._cached !== null) throw new Error("_cached: " + o._cached);     // not raw-0
if (o._cached === 0) throw new Error("_cached is 0");
if (o._count !== 7) throw new Error("_count: " + o._count);
if (JSON.stringify(o._list) !== "[1,2]") throw new Error("_list: " + JSON.stringify(o._list));
if (JSON.stringify(o._def) !== '{"k":1}') throw new Error("_def (super): " + JSON.stringify(o._def));
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: subclass own field init after super"
