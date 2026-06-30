#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/source.ts" <<'TS'
export enum Kind {
  Object = "Object",
  Readonly = "Readonly",
}

export enum Color {
  Red,
  Green,
}
TS

cat >"$TMPDIR/barrel.ts" <<'TS'
import * as z from "./source.js";
export * from "./source.js";
export { z };
TS

cat >"$TMPDIR/main.ts" <<'TS'
import { z, Kind, Color } from "./barrel.js";

const actual = {
  directKindType: typeof Kind,
  directKind: Kind.Object,
  namespaceKindType: typeof z.Kind,
  namespaceKind: z.Kind.Object,
  directColorType: typeof Color,
  directColor: Color.Green,
  reverseColor: Color[1],
  namespaceColorType: typeof z.Color,
  namespaceColor: z.Color.Red,
  keys: Object.keys(z.Color).sort(),
};

console.log(JSON.stringify(actual));
TS

"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/main.ts" \
    -o "$TMPDIR/main" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$TMPDIR/main" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

EXPECTED='{"directKindType":"object","directKind":"Object","namespaceKindType":"object","namespaceKind":"Object","directColorType":"object","directColor":1,"reverseColor":"Green","namespaceColorType":"object","namespaceColor":0,"keys":["0","1","Green","Red"]}'
ACTUAL="$(cat "$TMPDIR/run.log")"

if [[ "$ACTUAL" != "$EXPECTED" ]]; then
    echo "FAIL: output mismatch"
    echo "expected: $EXPECTED"
    echo "actual:   $ACTUAL"
    exit 1
fi

echo "PASS: export enum runtime object"
