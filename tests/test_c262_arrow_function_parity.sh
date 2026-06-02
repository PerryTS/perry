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

cat >"$TMPDIR/c262_arrow_parity.ts" <<'TS'
let failures = 0;

function check(label: string, actual: any, expected: any): void {
  if (actual !== expected) {
    console.log(label + ": expected " + String(expected) + ", got " + String(actual));
    failures = failures + 1;
  }
}

function checkThrowsTypeError(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": expected TypeError");
    failures = failures + 1;
  } catch (e) {
    if (!(e instanceof TypeError) && e.name !== "TypeError") {
      console.log(label + ": expected TypeError, got " + String(e && e.name));
      failures = failures + 1;
    }
  }
}

let captured = 1;
const capturedArrow = () => captured;
captured = 10;
check("closure capture observes later write", capturedArrow(), 10);

function makeReader(value: any): () => any {
  let local = value;
  return () => local;
}
check("returned arrow captures function local", makeReader("local")(), "local");

class Base {
  value(): string {
    return "base";
  }
}

class Derived extends Base {
  value(): string {
    const read = () => super.value();
    return read() + "-derived";
  }
}
check("arrow captures lexical super", new Derived().value(), "base-derived");

function Plain(): any {
  const direct = () => new.target;
  if (direct() === Plain) {
    this.constructed = true;
  }
  this.returnedArrow = () => new.target;
}

const instance = new Plain();
check("arrow sees new.target during constructor", instance.constructed, true);
check("returned arrow keeps lexical new.target", instance.returnedArrow(), Plain);

function NotConstructed(): any {
  const read = () => new.target;
  return read();
}
check("ordinary call has undefined new.target", NotConstructed(), undefined);

const arrow = () => {};
check("direct anonymous arrow name", (() => {}).name, "");
check("arrow default length 0", ((x = 1) => x).length, 0);
check("arrow length stops before default", ((x: any, y = 1) => x).length, 1);
check("arrow typeof", typeof arrow, "function");
check("arrow prototype chain", Object.getPrototypeOf(arrow), Function.prototype);
check("arrow has no own prototype", "prototype" in arrow, false);

checkThrowsTypeError("arrow caller getter", () => {
  arrow.caller;
});
checkThrowsTypeError("arrow caller setter", () => {
  arrow.caller = 1;
});
checkThrowsTypeError("arrow arguments getter", () => {
  arrow.arguments;
});
checkThrowsTypeError("arrow arguments setter", () => {
  arrow.arguments = 1;
});
checkThrowsTypeError("arrow is not constructor", () => {
  new arrow();
});
checkThrowsTypeError("inline arrow is not constructor", () => {
  new (() => {})();
});

if (failures !== 0) {
  throw new Error("c262 arrow parity regression failed");
}

console.log("c262 arrow parity ok");
TS

"$PERRY" compile --no-cache "$TMPDIR/c262_arrow_parity.ts" -o "$TMPDIR/c262_arrow_parity" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

"$TMPDIR/c262_arrow_parity" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "c262 arrow parity ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: c262 arrow parity"
