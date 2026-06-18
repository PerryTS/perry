#!/usr/bin/env bash
# Regression: a getter installed via `Object.defineProperty(Class.prototype,
# name, { get })` must run with `this` bound to the INSTANCE, not the prototype
# object it lives on.
#
# Such a getter is an ordinary method closure whose body reads `this` from its
# captured receiver slot (not IMPLICIT_THIS). The inherited-accessor walk used
# to merely set IMPLICIT_THIS and call the closure, so the getter observed the
# prototype — `this.<ownField>` came back undefined. winston's
# `Object.defineProperty(Logger.prototype, 'transports', { get() { const {
# pipes } = this._readableState; … } })` (read as `this.transports` inside the
# Logger constructor) then threw "Cannot convert undefined or null to object".
#
# Fix: the inherited prototype-accessor path routes through
# `invoke_accessor_getter`, which clones the getter closure with `this` rebound
# to the real receiver (matching the own-accessor read path).
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

SRC="$TMPDIR/defineproperty_proto_getter.ts"
BIN="$TMPDIR/defineproperty_proto_getter"

cat >"$SRC" <<'TS'
let failures = 0;

// Plain class: getter reads an own data field through `this`.
class A {
  _x = 42;
}
Object.defineProperty(A.prototype, "px", {
  get() { return (this as any)._x; },
});
const a: any = new A();
if (a.px !== 42) {
  console.log("FAIL: defineProperty prototype getter read prototype, not instance (px=" + a.px + ")");
  failures = failures + 1;
}

// Getter that DESTRUCTURES a nested own object — the exact winston shape.
class L {
  _readableState: any;
  constructor() { this._readableState = { pipes: null }; }
}
Object.defineProperty(L.prototype, "transports", {
  configurable: false,
  enumerable: true,
  get() {
    const { pipes } = (this as any)._readableState;
    return !Array.isArray(pipes) ? [pipes].filter(Boolean) : pipes;
  },
});
const l: any = new L();
let threw = false;
let result: any = "unset";
try {
  result = l.transports;
} catch (e) {
  threw = true;
}
if (threw) {
  console.log("FAIL: destructuring getter threw (this bound to prototype, _readableState undefined)");
  failures = failures + 1;
} else if (JSON.stringify(result) !== "[]") {
  console.log("FAIL: destructuring getter wrong result: " + JSON.stringify(result));
  failures = failures + 1;
}

if (failures !== 0) {
  throw new Error("defineProperty prototype getter this-binding regression failed");
}
console.log("defineProperty proto getter this ok");
TS

"$PERRY" compile --no-cache --no-auto-optimize "$SRC" -o "$BIN" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$BIN" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "defineProperty proto getter this ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: defineProperty prototype getter this-binding"
