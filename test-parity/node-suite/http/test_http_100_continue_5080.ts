// Issue #5080 — node:http 100-continue handshake, end-to-end.
//
// Client: `Expect: 100-continue` withholds the body until the server's
// interim `100 Continue` drives the request's `'continue'` event; the
// handler then sends the body. Server: a `'checkContinue'` listener fires
// *instead of* `'request'`, calls `res.writeContinue()`, then reads the
// body and responds.
//
// Data chunks are coerced with `.toString()` (the convention used across
// the http client/server test suite — see test_gap_http_overloads_3226plus
// / test_issue_1124_client_buffer). The implicit `b += chunk` form trips a
// separate, pre-existing client-side Buffer→string coercion crash that is
// unrelated to the 100-continue flow.

import http from "node:http";

const server = http.createServer((req, res) => {
  let b = "";
  req.on("data", (c: any) => (b += c.toString()));
  req.on("end", () => res.end("got:" + b));
});
server.on("checkContinue", (req: any, res: any) => {
  res.writeContinue();
  let b = "";
  req.on("data", (c: any) => (b += c.toString()));
  req.on("end", () => res.end("continue:" + b));
});
server.listen(0, () => {
  const port = (server.address() as any).port;
  const req = http.request(
    { port, method: "POST", headers: { Expect: "100-continue" } },
    (res: any) => {
      let b = "";
      res.on("data", (c: any) => (b += c.toString()));
      res.on("end", () => {
        console.log("body", b);
        server.close(() => console.log("closed"));
      });
    },
  );
  req.on("continue", () => {
    console.log("continue event");
    req.end("payload");
  });
});
setTimeout(() => {}, 1500);
