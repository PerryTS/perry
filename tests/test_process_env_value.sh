#!/usr/bin/env bash
set -euo pipefail

# `process.env` read as a whole VALUE (not the member form `process.env.X`) must
# materialize the live environment object. Member reads are special-cased to
# `EnvGet`, but passing `process.env` whole — e.g. `Schema.safeParse(process.env)`,
# the canonical config-validation pattern (zod/envalid/…) — reached the GlobalGet
# value fall-through and lowered to `undefined`, so the consumer iterated undefined.

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

cat >"$TMPDIR/e.ts" <<'TS'
const env: any = process.env;
if (typeof env !== "object" || env === null) throw new Error("typeof process.env: " + typeof env);
if (env.PERRY_TEST_VAR !== "hello") throw new Error("value: " + env.PERRY_TEST_VAR);
const keys = Object.keys(env);
if (keys.length < 1) throw new Error("no keys");
// passing process.env whole to a function (the safeParse(process.env) shape)
function count(o: Record<string, string>): number { return Object.keys(o).length; }
if (count(process.env as any) < 1) throw new Error("passed-whole count 0");
console.log("OK");
TS

OUT="$(PERRY_TEST_VAR=hello "$PERRY" run "$TMPDIR/e.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: process.env as a value"
