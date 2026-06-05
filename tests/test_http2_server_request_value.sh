#!/usr/bin/env bash
set -euo pipefail

# `Http2ServerRequest` / `Http2ServerResponse` imported as VALUES from
# `node:http2` are used by libraries purely for `x instanceof Http2ServerRequest`
# brand checks (e.g. @hono/node-server distinguishing HTTP/2 from HTTP/1
# requests). They resolved to `undefined`, so the `instanceof` RHS was not an
# object and threw "Right-hand side of 'instanceof' is not an object", 400'ing
# every request. They must be callable class values (so `instanceof` returns
# `false` for non-HTTP/2 values without throwing).

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/f.ts" <<'TS'
import { Http2ServerRequest, Http2ServerResponse } from "node:http2";

if (typeof Http2ServerRequest !== "function") {
  throw new Error("Http2ServerRequest typeof: " + typeof Http2ServerRequest);
}
if (typeof Http2ServerResponse !== "function") {
  throw new Error("Http2ServerResponse typeof: " + typeof Http2ServerResponse);
}
// instanceof must not throw and must be false for ordinary values.
const someObj: any = { url: "/x" };
if (someObj instanceof Http2ServerRequest) throw new Error("plain obj matched Http2ServerRequest");
if ((42 as any) instanceof Http2ServerResponse) throw new Error("number matched Http2ServerResponse");
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: node:http2 Http2ServerRequest/Response are callable for instanceof"
