#!/usr/bin/env bash
set -euo pipefail

# Regression: a SYMBOL-keyed property write on a native node:http
# `IncomingMessage` handle (e.g. @hono/node-server's
# `incoming[wrapBodyStream] = true` on the POST/PUT body path) used to throw
# `TypeError: Cannot assign to read only property 'property'` under strict mode,
# because `set_handle_property` reported the symbol write as a failed [[Set]].
# That rejected write surfaced as Hono's `handleFetchError` returning a 500 for
# every POST/PUT (GET/HEAD have no body and skip the write), so the native HTTP
# server replied 500 to all body-carrying requests before the route ran.
#
# Fix: route symbol-keyed writes on a native handle into the per-object symbol
# side table (and read them back from it). This test drives a real node:http
# server with a POST body and asserts the symbol round-trips AND the body is
# delivered to the handler.

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

cat >"$TMPDIR/srv.ts" <<'TS'
import * as http from "node:http";

const wrapBodyStream = Symbol("wrapBodyStream");

const server = http.createServer((req: any, res: any) => {
  // The @hono/node-server adapter does exactly this on every non-GET/HEAD
  // request. It must not throw, and must round-trip through the side table.
  let symOk = false;
  try {
    req[wrapBodyStream] = true;
    symOk = req[wrapBodyStream] === true;
  } catch (e: any) {
    res.statusCode = 500;
    res.end("SYMBOL_THREW:" + (e && e.message));
    return;
  }

  let body = "";
  req.on("data", (d: any) => { body += d; });
  req.on("end", () => {
    res.statusCode = 200;
    res.end("sym=" + symOk + ";body=" + body);
  });
});

server.listen(0, () => {
  const port = (server.address() as any).port;
  const data = "hello-post-body";
  const r = http.request(
    { host: "127.0.0.1", port, path: "/", method: "POST",
      headers: { "content-type": "text/plain", "content-length": data.length } },
    (resp: any) => {
      let out = "";
      resp.setEncoding("utf8");
      resp.on("data", (c: any) => { out += c; });
      resp.on("end", () => {
        console.log("STATUS=" + resp.statusCode);
        console.log("RESP=" + out);
        server.close();
      });
    },
  );
  r.write(data);
  r.end();
});
TS

# Run inside the tmpdir so `perry run` does not leave its compiled artifact
# (e.g. `srv`) in the caller's cwd / the repo tree.
OUT="$(cd "$TMPDIR" && "$PERRY" run "$TMPDIR/srv.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }

if ! grep -q "^STATUS=200$" <<<"$OUT"; then
  echo "FAIL: expected POST status 200, got:"; echo "$OUT"; exit 1;
fi
if ! grep -q "^RESP=sym=true;body=hello-post-body$" <<<"$OUT"; then
  echo "FAIL: symbol round-trip or POST body wrong, got:"; echo "$OUT"; exit 1;
fi
echo "PASS: symbol-keyed write on a native IncomingMessage handle round-trips; POST body delivered"
