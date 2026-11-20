// Regression: Node's `http.get`/`http.request` client must NOT follow HTTP
// redirects — a 3xx response is delivered to the caller verbatim (only `fetch`
// follows, per its WHATWG redirect mode). Perry's Node http client is built on
// reqwest, whose DEFAULT policy silently follows up to 10 hops. That auto-follow
// is observably wrong (the caller sees the final 200 instead of the 307) and,
// worse, made Next.js's `proxyRequest` (its bundled `http-proxy` runs over this
// client) loop forever: a proxied sub-request that 307-redirects back to the
// entry path was followed instead of relayed, so the router re-resolved the same
// locale-middleware rewrite endlessly (`/` rewrites to `/en`, `/en` 307s to `/`).
//
// The observable: the client must report the 307 and the `location` header, and
// the server must be hit exactly ONCE — byte-for-byte identical to
// `node --experimental-strip-types`.

import { createServer, get } from "node:http";

const PORT = 18993;
let serverHits = 0;

const server = createServer((req: any, res: any) => {
  serverHits++;
  if (req.url === "/target") {
    res.statusCode = 200;
    res.end("FINAL");
    return;
  }
  res.statusCode = 307;
  res.setHeader("location", `http://127.0.0.1:${PORT}/target`);
  res.end("redirect-body");
});

server.listen(PORT, () => {
  get(`http://127.0.0.1:${PORT}/start`, (res: any) => {
    let body = "";
    res.setEncoding("utf8");
    res.on("data", (c: string) => (body += c));
    res.on("end", () => {
      console.log(`status=${res.statusCode}`);
      console.log(`location=${res.headers.location}`);
      console.log(`body=${body}`);
      console.log(`serverHits=${serverHits}`);
      console.log(`followed=${res.statusCode === 200}`);
      server.close();
    });
  });
});
