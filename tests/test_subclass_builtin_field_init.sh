#!/usr/bin/env bash
set -euo pipefail

# A no-own-constructor subclass of a BUILT-IN (Error, etc.) must still run its
# OWN field initializers. The implicit ctor is
# `constructor(...args){ super(...args); <own field inits> }`.
#
# The field-init chain only contains USER classes, so a built-in parent is not
# in it: `class A extends Error {}` has chain `["A"]` (length 1). The no-own-ctor
# path applied `FieldInitMode::AfterRoot`, which keeps `chain[1..]` — EMPTY for a
# length-1 chain — so A's own field initializers never ran. Fields read the
# raw-0 slot; a later `this.arr.includes(x)` on an unset array field then threw
# `Cannot read properties of undefined (reading 'includes')`. (A two-level
# `class B extends A extends Error` worked because its chain is `["A","B"]`.)
#
# Fix: when the chain has no user root to skip (built-in / imported parent),
# `AfterRoot` applies the leaf's own initializers.

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
class A extends Error {
  pubv = 42;          // public field initializer
  #priv = 7;          // private field initializer
  arr = [1, 2, 3];    // reference-typed field
  readPub() { return this.pubv; }
  readPriv() { return this.#priv; }
  readArr() { return this.arr; }
}
const a = new A();
if (a.readPub() !== 42) throw new Error("pubv: " + a.readPub());
if (a.readPriv() !== 7) throw new Error("priv: " + a.readPriv());
if (!a.readArr().includes(2)) throw new Error("arr uninitialized: " + JSON.stringify(a.readArr()));
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: subclass-of-builtin own field initializers run"
