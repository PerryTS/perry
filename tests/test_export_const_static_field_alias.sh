#!/usr/bin/env bash
set -euo pipefail

# A module-level const that aliases a class STATIC field value, re-exported via a
# separate `export { x }` clause, must be readable + callable cross-module.
# Before the fix its init lowered to `Expr::StaticFieldGet`, which was missing
# from the separate-clause export's `is_exportable` set — so the const never got
# a backing module global and the importer link-resolved a nonexistent
# `perry_fn_<src>__<name>` symbol, making the call return `undefined`.
# zod's `z.string`/`z.number`/… are exactly this shape:
#   const stringType = ZodString.create; export { stringType as string }

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

cat >"$TMPDIR/types.ts" <<'TS'
export class ZS {
  v = 5;
  static create = (): ZS => { return new ZS(); };
}
const stringType = ZS.create;          // const aliasing a static arrow field
export { stringType as string };        // separate-clause re-export with alias
TS
cat >"$TMPDIR/barrel.ts" <<'TS'
import * as z from "./types.js";
export { z };
TS
cat >"$TMPDIR/main.ts" <<'TS'
import { z } from "./barrel.js";
import { string as direct } from "./types.js";
const a = (z as any).string();
if (!a || (a as any).v !== 5) throw new Error("via namespace: " + JSON.stringify(a));
const b = (direct as any)();
if (!b || (b as any).v !== 5) throw new Error("direct import: " + JSON.stringify(b));
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/main.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: export const static-field alias"
