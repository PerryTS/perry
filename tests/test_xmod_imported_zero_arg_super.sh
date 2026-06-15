#!/bin/bash
# Regression: an explicit zero-argument constructor on an imported superclass
# must run when a local subclass calls super().

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

cat > "$TMPDIR/root.ts" <<'TS'
export class CrossRoot {
  constructor() {
    (this as any).rootReady = "root";
  }
}
TS

cat > "$TMPDIR/base.ts" <<'TS'
import { CrossRoot } from "./root.ts";

export class CrossBase extends CrossRoot {
  constructor() {
    super();
    (this as any).baseReady = "base";
  }
}
TS

cat > "$TMPDIR/main.ts" <<'TS'
import { CrossBase } from "./base.ts";

class CrossSub extends CrossBase {
  constructor() {
    super();
    (this as any).subReady = "sub";
  }
}

const value: any = new CrossSub();
console.log("instanceof base", value instanceof CrossBase);
console.log("root", value.rootReady);
console.log("base", value.baseReady);
console.log("sub", value.subReady);
TS

cd "$TMPDIR"
"$PERRY" compile --no-cache --no-auto-optimize main.ts --output test_bin >/dev/null
RUN_OUTPUT="$(./test_bin 2>&1)"

EXPECTED="instanceof base true
root root
base base
sub sub"

if [[ "$RUN_OUTPUT" == "$EXPECTED" ]]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: imported zero-arg superclass constructor was not replayed"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
