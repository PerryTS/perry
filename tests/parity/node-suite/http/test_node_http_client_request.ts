// Issue #769 — `http.request` / `http.get` were unreachable from
// compiled code because no `NativeModSig` dispatch entry existed; the
// runtime implementations existed in perry-ext-http but the call site
// returned `TAG_UNDEFINED` and crashed on `req.on(...)`.
//
// This fixture exercises the client surface end-to-end against a
// node:http server spun up in the same process: namespace-import
// `request` and `get`, URL-string and options-object overloads, the
// `(res) => ...` response callback, and the `'response'` / `'error'`
// listener registration on the returned ClientRequest handle.

import { createServer, request, get } from "node:http";

const port = 18889;

const server = createServer((_req: any, res: any) => {
  res.setHeader("Content-Type", "text/plain");
  res.end("hello");
});

server.listen(port, () => {
  console.log("server listening");

  // Chain the requests so the parity comparison against
  // `node --experimental-strip-types` sees a deterministic line order.
  // Parallel requests would race the three "status" prints and the
  // line order would flip run-to-run.
  //
  // (1) http.request(url, cb) — URL-string overload (the form the
  //     issue #769 reporter used).
  const req1 = request("http://127.0.0.1:" + port + "/a", (res: any) => {
    console.log("req1 status:", res.statusCode);

    // (2) http.request(options, cb) — options-object overload.
    const req2 = request(
      { host: "127.0.0.1", port: port, path: "/b", method: "GET" },
      (res2: any) => {
        console.log("req2 status:", res2.statusCode);

        // (3) http.get(url, cb) — convenience form (auto-ends).
        const req3 = get("http://127.0.0.1:" + port + "/c", (res3: any) => {
          console.log("req3 status:", res3.statusCode);
          server.close();
          console.log("done");
        });
        req3.on("error", (_err: any) => { console.log("req3 error fired"); });
      },
    );
    req2.on("error", (_err: any) => { console.log("req2 error fired"); });
    req2.end();
  });
  req1.on("error", (_err: any) => { console.log("req1 error fired"); });
  req1.end();
});
