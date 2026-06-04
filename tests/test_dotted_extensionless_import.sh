#!/usr/bin/env bash
# Regression: `import "./name.gen"` must resolve to `name.gen.ts`, not `name.ts`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
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

cat >"$TMPDIR/migration.gen.ts" <<'JS'
export const migrations = [41]
JS

cat >"$TMPDIR/migration.ts" <<'JS'
import { migrations } from "./migration.gen"

export function apply() {
  return migrations[0] + 1
}

console.log(apply())
JS

set +e
env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-auto-optimize \
    "$TMPDIR/migration.ts" -o "$TMPDIR/dotted-import" \
    >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -ne 0 ]]; then
    echo "FAIL: dotted extensionless import failed to compile"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

output="$("$TMPDIR/dotted-import")"
if [[ "$output" != "42" ]]; then
    echo "FAIL: expected 42, got: $output"
    exit 1
fi

echo "PASS"
