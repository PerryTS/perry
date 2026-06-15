#!/bin/bash
# Regression: an imported derived class with no own constructor and no fields
# must not stop constructor dispatch. Args should still reach the first
# effectful imported ancestor constructor.

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
export class EffectfulRoot {
  constructor(value: any) {
    (this as any).forwarded = value;
  }
}
TS

cat > "$TMPDIR/mid.ts" <<'TS'
import { EffectfulRoot } from "./root.ts";

export class EmptyMid extends EffectfulRoot {}
TS

cat > "$TMPDIR/main.ts" <<'TS'
import { EmptyMid } from "./mid.ts";

const direct: any = new EmptyMid("direct");
console.log("direct instanceof mid", direct instanceof EmptyMid);
console.log("direct forwarded", direct.forwarded);

class ExplicitLeaf extends EmptyMid {
  constructor(value: any) {
    super(value);
    (this as any).leafReady = "explicit";
  }
}

const explicit: any = new ExplicitLeaf("explicit");
console.log("explicit instanceof mid", explicit instanceof EmptyMid);
console.log("explicit forwarded", explicit.forwarded);
console.log("explicit ready", explicit.leafReady);

class DefaultLeaf extends EmptyMid {}

const defaultLeaf: any = new DefaultLeaf("default");
console.log("default instanceof mid", defaultLeaf instanceof EmptyMid);
console.log("default forwarded", defaultLeaf.forwarded);
TS

cd "$TMPDIR"
"$PERRY" compile --no-cache --no-auto-optimize main.ts --output test_bin >/dev/null
RUN_OUTPUT="$(./test_bin 2>&1)"

EXPECTED="direct instanceof mid true
direct forwarded direct
explicit instanceof mid true
explicit forwarded explicit
explicit ready explicit
default instanceof mid true
default forwarded default"

if [[ "$RUN_OUTPUT" == "$EXPECTED" ]]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: empty imported derived class did not forward args to effectful ancestor"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
