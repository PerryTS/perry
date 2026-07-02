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

cat >"$TMPDIR/main.ts" <<'TS'
let enabled = true;
try {
  new Function("");
} catch {
  enabled = false;
}
console.log("empty feature", enabled ? "dynamic" : "fallback");

enabled = true;
try {
  const body = String(Math.random()).slice(100);
  const fn: any = new Function(body);
  fn();
} catch {
  enabled = false;
}
console.log("feature", enabled ? "dynamic" : "fallback");

try {
  const Ctor: any = Function;
  const body = ["return", "7"].join(" ");
  new Ctor(body);
} catch {
  console.log("reflected fallback");
}
TS

COMPILE_ARGS=(compile --no-cache)
if [[ -f "$REPO_ROOT/target/debug/libperry_runtime.a" && -f "$REPO_ROOT/target/debug/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/debug"
    COMPILE_ARGS+=(--no-auto-optimize)
elif [[ -f "$REPO_ROOT/target/release/libperry_runtime.a" && -f "$REPO_ROOT/target/release/libperry_stdlib.a" ]]; then
    export PERRY_RUNTIME_DIR="$REPO_ROOT/target/release"
    COMPILE_ARGS+=(--no-auto-optimize)
fi

COMPILE_OUT="$(PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY" "${COMPILE_ARGS[@]}" "$TMPDIR/main.ts" -o "$TMPDIR/out" 2>&1)" || {
    echo "FAIL: perry compile errored"
    echo "$COMPILE_OUT"
    exit 1
}

"$TMPDIR/out" >"$TMPDIR/stdout.log" 2>"$TMPDIR/stderr.log" || {
    echo "FAIL: compiled binary errored"
    sed 's/^/    /' "$TMPDIR/stdout.log"
    sed 's/^/    /' "$TMPDIR/stderr.log"
    exit 1
}

cat >"$TMPDIR/expected.log" <<'EOF_EXPECTED'
empty feature fallback
feature fallback
reflected fallback
EOF_EXPECTED

if ! diff -u "$TMPDIR/expected.log" "$TMPDIR/stdout.log"; then
    echo "FAIL: stdout mismatch"
    exit 1
fi

if [[ -s "$TMPDIR/stderr.log" ]]; then
    echo "FAIL: expected empty stderr"
    sed 's/^/    /' "$TMPDIR/stderr.log"
    exit 1
fi

echo "PASS: dynamic Function feature probe fallback is silent"
