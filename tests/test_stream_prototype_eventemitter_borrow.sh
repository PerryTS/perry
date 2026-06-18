#!/usr/bin/env bash
# Regression: `Stream.prototype.<eeMethod>.call(this, …)` borrow.
#
# readable-stream's `Readable.prototype.on` is literally:
#   Readable.prototype.on = function (ev, fn) {
#     var res = Stream.prototype.on.call(this, ev, fn);  // <-- line 745
#     ...
#   };
# where `Stream = require('stream')` (in Node `require('stream')` IS the legacy
# `Stream` constructor, so `Stream.prototype.on` is EventEmitter.prototype.on).
#
# Perry models `require('stream')` as a namespace OBJECT, so both
# `require('stream').prototype` and `require('stream').Stream.prototype` lacked
# the EventEmitter methods — `Stream.prototype.on` was `undefined` and the
# borrow threw "Function.prototype.call was called on a value that is not a
# function". This wall blocked winston (`class Logger extends Transform`).
#
# Fix:
#   - the legacy `Stream.prototype` (and Readable/Writable/Duplex/Transform/
#     PassThrough prototypes) now expose the EventEmitter methods as
#     receiver-from-`this` values, and
#   - `require('stream').prototype` resolves to that same legacy prototype.
#
# This test borrows `on`/`emit` off both forms and verifies they register and
# dispatch the listener against the call-site `this`.
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

SRC="$TMPDIR/stream_proto_ee_borrow.ts"
BIN="$TMPDIR/stream_proto_ee_borrow"

cat >"$SRC" <<'TS'
const Stream: any = require("stream");

let failures = 0;

// Form A: the namespace itself (`require('stream').prototype`), the exact shape
// readable-stream borrows from.
if (typeof Stream.prototype !== "object" || typeof Stream.prototype.on !== "function") {
  console.log("FAIL: require('stream').prototype.on is not a function");
  failures = failures + 1;
} else {
  const a: any = {};
  let gotA: any = null;
  Stream.prototype.on.call(a, "data", (x: any) => { gotA = x; });
  Stream.prototype.emit.call(a, "data", 11);
  if (gotA !== 11) { console.log("FAIL: namespace borrow lost the listener"); failures = failures + 1; }
}

// Form B: the `Stream` constructor export (`require('stream').Stream`).
const SS: any = Stream.Stream;
if (typeof SS.prototype.on !== "function" || typeof SS.prototype.removeListener !== "function") {
  console.log("FAIL: Stream.prototype EE methods missing");
  failures = failures + 1;
} else {
  const b: any = {};
  let count = 0;
  const fn = () => { count = count + 1; };
  SS.prototype.on.call(b, "x", fn);
  SS.prototype.emit.call(b, "x");
  SS.prototype.removeListener.call(b, "x", fn);
  SS.prototype.emit.call(b, "x");
  if (count !== 1) { console.log("FAIL: removeListener borrow did not unregister (count=" + count + ")"); failures = failures + 1; }
}

if (failures !== 0) {
  throw new Error("Stream.prototype EventEmitter borrow regression failed");
}
console.log("stream proto ee borrow ok");
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

if ! grep -q "stream proto ee borrow ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: stream prototype EventEmitter borrow"
