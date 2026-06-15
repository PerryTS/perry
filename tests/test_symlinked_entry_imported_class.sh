#!/bin/bash
# Regression: resolving static imports from a symlinked entry path must use the
# source-visible importer path before the canonical path. Otherwise a valid
# relative import can be missed, and `new ImportedClass()` falls back to an
# empty class-id-0 placeholder with no fields, methods, or instanceof identity.

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
if [[ "$PERRY" != /* ]]; then
  PERRY="$(cd "$(dirname "$PERRY")" && pwd)/$(basename "$PERRY")"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

mkdir -p "$TMPDIR/real-parent/app" "$TMPDIR/alias-parent/outside" "$TMPDIR/real-parent/outside"
ln -s "$TMPDIR/real-parent/app" "$TMPDIR/alias-parent/app"

cat > "$TMPDIR/alias-parent/outside/dep.ts" <<'TS'
export class ExternalCtor {
  value: string;

  constructor(value: string) {
    this.value = value;
  }

  marker(): string {
    return this.value;
  }
}
TS

cat > "$TMPDIR/real-parent/outside/dep.ts" <<'TS'
export class ExternalCtor {
  value: string;

  constructor(value: string) {
    this.value = "decoy:" + value;
  }

  marker(): string {
    return this.value;
  }
}
TS

cat > "$TMPDIR/alias-parent/app/child.ts" <<'TS'
import { ExternalCtor } from "../outside/dep";

export function makeValue(): any {
  return new ExternalCtor("ready");
}
TS

cat > "$TMPDIR/alias-parent/app/main.ts" <<'TS'
import { ExternalCtor } from "../outside/dep";
import { makeValue } from "./child";

const value: any = makeValue();
console.log("field", value.value);
console.log("method", typeof value.marker);
console.log("call", value.marker());
console.log("instanceof", value instanceof ExternalCtor);
TS

BIN="$TMPDIR/test_bin"
"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/alias-parent/app/main.ts" --output "$BIN" >/dev/null
set +e
RUN_OUTPUT="$("$BIN" 2>&1)"
RUN_STATUS=$?
set -e

EXPECTED="field ready
method function
call ready
instanceof true"

if [[ "$RUN_STATUS" -eq 0 && "$RUN_OUTPUT" == "$EXPECTED" ]]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: symlinked entry relative import did not preserve imported class behavior"
echo "Exit status: $RUN_STATUS"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
