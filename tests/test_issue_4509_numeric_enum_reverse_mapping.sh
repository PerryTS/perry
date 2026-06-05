#!/usr/bin/env bash
set -euo pipefail

# Issue #4509: TypeScript numeric enums generate a reverse mapping
# (`E[value] === "Name"`). Perry emitted the forward members but the
# reverse lookup returned `undefined` — a silent miscompile. This test
# pins both directions: numeric enums reverse-map (dynamic + literal
# index), the forward computed form still works, and string enums stay
# one-directional (no reverse entry), matching tsc.

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

cat >"$TMPDIR/enum_reverse.ts" <<'TS'
enum Color { Red, Green, Blue }
enum Dir { Up = 1, Down }
enum S { A = "aaa", B = "bbb" }

const c: Color = Color.Blue;

// Reverse mapping (numeric): dynamic index and literal indices.
if (Color[c] !== "Blue") throw new Error(`reverse dyn: ${Color[c]}`);
if (Color[0] !== "Red" || Color[1] !== "Green" || Color[2] !== "Blue") {
  throw new Error(`reverse lit: ${Color[0]},${Color[1]},${Color[2]}`);
}
if (Dir[1] !== "Up" || Dir[2] !== "Down") {
  throw new Error(`reverse explicit-base: ${Dir[1]},${Dir[2]}`);
}

// Forward mapping still works through the computed form.
if ((Color["Blue"] as number) !== 2 || (Dir["Down"] as number) !== 2) {
  throw new Error(`forward computed: ${Color["Blue"]},${Dir["Down"]}`);
}

// Forward `.Member` constant fold is unaffected.
if (Color.Blue !== 2 || Dir.Down !== 2) {
  throw new Error(`forward member: ${Color.Blue},${Dir.Down}`);
}

// String enums are one-directional — no reverse entry.
if (S.A !== "aaa" || S["B"] !== "bbb") {
  throw new Error(`string forward: ${S.A},${S["B"]}`);
}
if ((S as any)["aaa"] !== undefined) {
  throw new Error(`string reverse should be undefined: ${(S as any)["aaa"]}`);
}

// Out-of-range numeric reverse lookup is undefined.
if ((Color as any)[99] !== undefined) {
  throw new Error(`oob reverse should be undefined: ${(Color as any)[99]}`);
}

console.log("numeric enum reverse mapping ok");
TS

"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/enum_reverse.ts" \
    -o "$TMPDIR/enum_reverse" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$TMPDIR/enum_reverse" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "numeric enum reverse mapping ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: numeric enum reverse mapping"
