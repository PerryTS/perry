#!/usr/bin/env bash
set -euo pipefail

# An object-literal SHORTHAND property whose name is a cross-module imported
# binding (`import { db } from "./client.js"; const ctx = { db, ...rest }`) must
# resolve to the imported value. The HIR shorthand-folding only checked
# local/func/class/builtin bindings, so an imported name fell through and the
# property was silently DROPPED — `{ db }` lowered to an empty object, so
# `getContext().db` was `undefined` and every `db.query.*` read crashed
# ("Cannot read properties of undefined"). Covers all three object-literal
# lowering paths: closed-shape, spread/method IIFE, and the computed-key path.

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

cat >"$TMPDIR/client.ts" <<'TS'
export const db = { tag: "DBINST", q: () => 7 };
export const helper = () => "H";
TS
cat >"$TMPDIR/main.ts" <<'TS'
import { db, helper } from "./client.js";

// 1) closed-shape shorthand
const a: any = { db };
if (a.db?.tag !== "DBINST") throw new Error("closed-shape: " + JSON.stringify(a.db));

// 2) shorthand + sibling (still closed-shape)
const b: any = { db, n: 1 };
if (b.db?.tag !== "DBINST" || b.n !== 1) throw new Error("sibling: " + JSON.stringify(b));

// 3) shorthand + spread (the getContext shape — IIFE path)
const rest = { x: 9 };
const c: any = { db, helper, ...rest };
if (c.db?.tag !== "DBINST") throw new Error("spread db: " + JSON.stringify(c.db));
if (typeof c.helper !== "function" || c.helper() !== "H") throw new Error("spread helper");
if (c.x !== 9) throw new Error("spread x: " + c.x);

// 4) shorthand + computed key (computed-key path)
const key = "dyn";
const d: any = { db, [key]: 5 };
if (d.db?.tag !== "DBINST" || d.dyn !== 5) throw new Error("computed: " + JSON.stringify(d));

// the imported value is fully usable
if (a.db.q() !== 7) throw new Error("method call: " + a.db.q());
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/main.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: object shorthand resolves a cross-module imported binding"
